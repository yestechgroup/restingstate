use serde::ser::Serialize as _;
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use xstate_backend_native::NativeActorSystem;
use xstate_core::actions::{ActionImpl, ActionRegistry};
use xstate_core::actor_logic::ReducerLogic;
use xstate_core::backend::{ActorStatus, ActorSystem as BackendActorSystem};
use xstate_core::guards::{GuardImpl, GuardRegistry};
use xstate_core::{Interpreter, MockClock};
use xstate_ir::Machine;

thread_local! {
    static JS_GUARDS: RefCell<HashMap<String, js_sys::Function>> = RefCell::new(HashMap::new());
    static JS_ACTIONS: RefCell<HashMap<String, js_sys::Function>> = RefCell::new(HashMap::new());
}

fn json_to_js(val: &serde_json::Value) -> JsValue {
    val.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .unwrap_or(JsValue::NULL)
}

fn report_callback_error(kind: &str, name: &str, err: &JsValue, fallback: &str) {
    let detail = js_sys::Error::from(err.clone()).to_string();
    let console = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("console"))
        .unwrap_or(JsValue::UNDEFINED);
    let error_fn = js_sys::Reflect::get(&console, &JsValue::from_str("error"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
    if let Some(error_fn) = error_fn {
        let message = JsValue::from_str(&format!(
            "xstate-wasm: {kind} '{name}' threw: {detail}. {fallback}"
        ));
        let _ = error_fn.call1(&console, &message);
    }
}

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

fn js_guards_registry() -> GuardRegistry {
    let mut registry = GuardRegistry::new();
    let names: Vec<String> = JS_GUARDS.with(|guards| guards.borrow().keys().cloned().collect());
    for name in names {
        let lookup = name.clone();
        registry.register(GuardImpl::new(name, move |ctx, event| {
            JS_GUARDS.with(|guards| {
                let guards = guards.borrow();
                let Some(func) = guards.get(&lookup) else {
                    return true;
                };
                let ctx_val = json_to_js(ctx);
                let evt_val = json_to_js(event);
                match func.call2(&JsValue::NULL, &ctx_val, &evt_val) {
                    Ok(val) => val.as_bool().unwrap_or(true),
                    Err(err) => {
                        report_callback_error(
                            "guard",
                            &lookup,
                            &err,
                            "Allowing transition by default.",
                        );
                        true
                    }
                }
            })
        }));
    }
    registry
}

fn js_actions_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    let names: Vec<String> = JS_ACTIONS.with(|actions| actions.borrow().keys().cloned().collect());
    for name in names {
        let lookup = name.clone();
        registry.register(ActionImpl::new(name, move |args, event, _ctx| {
            JS_ACTIONS.with(|actions| {
                let actions = actions.borrow();
                let Some(func) = actions.get(&lookup) else {
                    return;
                };
                let args_val = json_to_js(&serde_json::Value::Array(args.to_vec()));
                let evt_val = json_to_js(event);
                if let Err(err) = func.call2(&JsValue::NULL, &args_val, &evt_val) {
                    report_callback_error("action", &lookup, &err, "Action skipped.");
                }
            })
        }));
    }
    registry
}

#[wasm_bindgen(js_name = registerGuard)]
pub fn register_guard(name: &str, func: &js_sys::Function) {
    JS_GUARDS.with(|guards| {
        guards.borrow_mut().insert(name.to_string(), func.clone());
    });
}

#[wasm_bindgen(js_name = registerAction)]
pub fn register_action(name: &str, func: &js_sys::Function) {
    JS_ACTIONS.with(|actions| {
        actions.borrow_mut().insert(name.to_string(), func.clone());
    });
}

#[wasm_bindgen(js_name = clearRegistry)]
pub fn clear_registry() {
    JS_GUARDS.with(|g| g.borrow_mut().clear());
    JS_ACTIONS.with(|a| a.borrow_mut().clear());
}

#[wasm_bindgen]
pub struct JsMachine {
    interpreter: Interpreter,
    state: RefCell<xstate_core::StateValue>,
    context: RefCell<serde_json::Value>,
}

#[wasm_bindgen]
impl JsMachine {
    #[wasm_bindgen(constructor)]
    pub fn new(machine_json: &str) -> Result<JsMachine, JsValue> {
        let machine: Machine = serde_json::from_str(machine_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid machine JSON: {}", e)))?;
        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        let context = interpreter.initial_context();
        Ok(JsMachine {
            interpreter,
            state: RefCell::new(state),
            context: RefCell::new(context),
        })
    }

    #[wasm_bindgen(js_name = snapshot)]
    pub fn snapshot(&self) -> String {
        let state_value = serde_json::to_value(&*self.state.borrow()).unwrap_or_default();
        let snap = serde_json::json!({
            "value": state_value,
            "context": self.context.borrow().clone(),
        });
        serde_json::to_string(&snap).unwrap_or_default()
    }

    #[wasm_bindgen(js_name = persistedSnapshot)]
    pub fn persisted_snapshot(&self) -> String {
        let state = self.state.borrow();
        let context = self.context.borrow();
        let snap = self.interpreter.persisted_snapshot(&state, &context);
        serde_json::to_string(&snap).unwrap_or_default()
    }

    #[wasm_bindgen(js_name = restore)]
    pub fn restore(machine_json: &str, snapshot_json: &str) -> Result<JsMachine, JsValue> {
        let machine: Machine = serde_json::from_str(machine_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid machine JSON: {}", e)))?;
        let snapshot: serde_json::Value = serde_json::from_str(snapshot_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid snapshot JSON: {}", e)))?;
        let interpreter = Interpreter::new(machine);
        let (state, context) = interpreter
            .restore_persisted(&snapshot)
            .map_err(|e| JsValue::from_str(&format!("Restore failed: {}", e)))?;
        Ok(JsMachine {
            interpreter,
            state: RefCell::new(state),
            context: RefCell::new(context),
        })
    }

    #[wasm_bindgen(js_name = send)]
    pub fn send(&self, event_json: &str) -> Option<String> {
        let event: serde_json::Value = serde_json::from_str(event_json).ok()?;

        let mut context = self.context.borrow_mut();
        let current_state = self.state.borrow().clone();

        let guards =
            |name: &str, ctx: &serde_json::Value, event_payload: &serde_json::Value| -> bool {
                JS_GUARDS.with(|guards| {
                    if let Some(func) = guards.borrow().get(name) {
                        let this = JsValue::NULL;
                        let ctx_val = json_to_js(ctx);
                        let evt_val = json_to_js(event_payload);
                        match func.call2(&this, &ctx_val, &evt_val) {
                            Ok(val) => val.as_bool().unwrap_or(true),
                            Err(err) => {
                                report_callback_error(
                                    "guard",
                                    name,
                                    &err,
                                    "Allowing transition by default.",
                                );
                                true
                            }
                        }
                    } else {
                        true
                    }
                })
            };

        let mut actions =
            |name: &str, args: &[serde_json::Value], event_payload: &serde_json::Value| {
                JS_ACTIONS.with(|actions| {
                    if let Some(func) = actions.borrow().get(name) {
                        let this = JsValue::NULL;
                        let args_val = json_to_js(&serde_json::Value::Array(args.to_vec()));
                        let evt_val = json_to_js(event_payload);
                        if let Err(err) = func.call2(&this, &args_val, &evt_val) {
                            report_callback_error("action", name, &err, "Action skipped.");
                        }
                    }
                });
            };

        let clock = MockClock::new(0);

        match self.interpreter.send(
            &current_state,
            &event,
            &mut context,
            &guards,
            &mut actions,
            &clock,
        ) {
            Some(new_state) => {
                *self.state.borrow_mut() = new_state;
                let state_value = serde_json::to_value(&*self.state.borrow()).unwrap_or_default();
                let snap = serde_json::json!({
                    "value": state_value,
                    "context": context.clone(),
                });
                Some(serde_json::to_string(&snap).unwrap_or_default())
            }
            None => None,
        }
    }

    #[wasm_bindgen(js_name = getContext)]
    pub fn get_context(&self) -> String {
        serde_json::to_string(&*self.context.borrow()).unwrap_or_default()
    }

    #[wasm_bindgen(js_name = getState)]
    pub fn get_state(&self) -> String {
        let state_value = serde_json::to_value(&*self.state.borrow()).unwrap_or_default();
        serde_json::to_string(&state_value).unwrap_or_default()
    }
}

#[wasm_bindgen]
pub struct JsActorSystem {
    system: NativeActorSystem,
}

impl Default for JsActorSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl JsActorSystem {
    #[wasm_bindgen(constructor)]
    pub fn new() -> JsActorSystem {
        let system = NativeActorSystem::new();
        system.set_registry_factory(|| (js_guards_registry(), js_actions_registry()));
        JsActorSystem { system }
    }

    #[wasm_bindgen(js_name = spawn)]
    pub fn spawn(&self, id: &str, machine_json: &str) -> Result<(), JsValue> {
        let machine: Machine = serde_json::from_str(machine_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid machine JSON: {}", e)))?;
        self.system.spawn_machine(id, machine);
        Ok(())
    }

    #[wasm_bindgen(js_name = registerMachine)]
    pub fn register_machine(&self, name: &str, machine_json: &str) -> Result<(), JsValue> {
        let machine: Machine = serde_json::from_str(machine_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid machine JSON: {}", e)))?;
        self.system.register_machine(name, machine);
        Ok(())
    }

    #[wasm_bindgen(js_name = spawnReducer)]
    pub fn spawn_reducer(&self, id: &str, initial_json: &str) -> Result<(), JsValue> {
        let initial: serde_json::Value = serde_json::from_str(initial_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid initial state JSON: {}", e)))?;
        let logic = ReducerLogic::new(id, initial, |state, _event| (state.clone(), vec![]));
        self.system.spawn(id, Box::new(logic));
        Ok(())
    }

    #[wasm_bindgen(js_name = spawnFromState)]
    pub fn spawn_from_state(
        &self,
        id: &str,
        machine_json: &str,
        snapshot_json: &str,
    ) -> Result<(), JsValue> {
        let machine: Machine = serde_json::from_str(machine_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid machine JSON: {}", e)))?;
        let snapshot: serde_json::Value = serde_json::from_str(snapshot_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid snapshot JSON: {}", e)))?;
        self.system
            .spawn_machine_from_persisted(id, machine, &snapshot)
            .map_err(|e| JsValue::from_str(&format!("Restore failed: {}", e)))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = childrenOf)]
    pub fn children_of(&self, id: &str) -> String {
        serde_json::to_string(&self.system.children_of(id)).unwrap_or_default()
    }

    #[wasm_bindgen(js_name = sendTo)]
    pub fn send_to(&self, id: &str, event_json: &str) {
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(event_json) {
            self.system.send_to(id, event);
        }
    }

    #[wasm_bindgen(js_name = snapshot)]
    pub fn snapshot(&self, id: &str) -> Option<String> {
        self.system
            .snapshot(id)
            .map(|s| serde_json::to_string(&s).unwrap_or_default())
    }

    #[wasm_bindgen(js_name = stop)]
    pub fn stop(&self, id: &str) {
        self.system.stop(id);
    }

    #[wasm_bindgen(js_name = status)]
    pub fn status(&self, id: &str) -> Option<String> {
        self.system.status(id).map(|s| {
            match s {
                ActorStatus::Running => "running",
                ActorStatus::Stopped => "stopped",
                ActorStatus::Error => "error",
                ActorStatus::Done => "done",
            }
            .to_string()
        })
    }

    #[wasm_bindgen(js_name = len)]
    pub fn len(&self) -> usize {
        self.system.len()
    }

    #[wasm_bindgen(js_name = isEmpty)]
    pub fn is_empty(&self) -> bool {
        self.system.is_empty()
    }
}
