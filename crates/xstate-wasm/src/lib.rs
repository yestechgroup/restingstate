use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use xstate_core::clock::Clock;
use xstate_core::Interpreter;
use xstate_ir::Machine;

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

struct WasmClock;

impl Clock for WasmClock {
    fn now_ms(&self) -> u64 {
        0
    }
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
        let snapshot = serde_json::json!({
            "value": state_value,
            "context": self.context.borrow().clone(),
        });
        serde_json::to_string(&snapshot).unwrap_or_default()
    }

    #[wasm_bindgen(js_name = send)]
    pub fn send(&self, event_json: &str) -> Option<String> {
        let event: serde_json::Value = serde_json::from_str(event_json).ok()?;
        let event_type = event["type"].as_str()?;
        let _ = event_type;

        let mut context = self.context.borrow_mut();
        let current_state = self.state.borrow().clone();

        let guards = |name: &str,
                      ctx: &serde_json::Value,
                      _event_payload: &serde_json::Value|
         -> bool {
            if let Some(val) = ctx.get(name) {
                matches!(val, serde_json::Value::Bool(true))
            } else {
                true
            }
        };

        let mut actions =
            |_name: &str, _args: &[serde_json::Value], _event_payload: &serde_json::Value| {};

        let clock = WasmClock;

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
                let state_value =
                    serde_json::to_value(&*self.state.borrow()).unwrap_or_default();
                let snapshot = serde_json::json!({
                    "value": state_value,
                    "context": context.clone(),
                });
                Some(serde_json::to_string(&snapshot).unwrap_or_default())
            }
            None => None,
        }
    }
}
