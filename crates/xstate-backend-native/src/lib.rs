use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use xstate_core::actor_logic::ActorLogic;
use xstate_core::backend::{ActorHandle, ActorStatus, ActorSystem as BackendActorSystem};

struct ActorState {
    logic: Box<dyn ActorLogic>,
    snapshot: serde_json::Value,
    status: ActorStatus,
}

type ActorMap = Mutex<HashMap<String, ActorState>>;

pub struct NativeActorSystem {
    actors: Arc<ActorMap>,
}

impl NativeActorSystem {
    pub fn new() -> Self {
        NativeActorSystem {
            actors: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn with_actors<R>(&self, f: impl FnOnce(&mut HashMap<String, ActorState>) -> R) -> R {
        let mut actors = self.actors.lock().unwrap();
        f(&mut actors)
    }

    pub fn dispatch(&self, id: &str, event: serde_json::Value) {
        self.with_actors(|actors| {
            if let Some(actor) = actors.get_mut(id) {
                if actor.status != ActorStatus::Running {
                    return;
                }
                let _effects = actor.logic.receive(event, &mut actor.snapshot);
            }
        });
    }

    pub fn stop_actor(&self, id: &str) {
        self.with_actors(|actors| {
            if let Some(actor) = actors.get_mut(id) {
                actor.logic.stop();
                actor.status = ActorStatus::Stopped;
            }
        });
    }

    pub fn get_snapshot(&self, id: &str) -> Option<serde_json::Value> {
        self.actors
            .lock()
            .unwrap()
            .get(id)
            .map(|a| a.snapshot.clone())
    }

    pub fn get_status(&self, id: &str) -> Option<ActorStatus> {
        self.actors.lock().unwrap().get(id).map(|a| a.status)
    }
}

impl Default for NativeActorSystem {
    fn default() -> Self {
        Self::new()
    }
}

pub struct NativeActorHandle {
    actors: Arc<ActorMap>,
    id: String,
}

impl ActorHandle for NativeActorHandle {
    fn send(&self, event: serde_json::Value) {
        let mut actors = self.actors.lock().unwrap();
        if let Some(actor) = actors.get_mut(&self.id) {
            if actor.status != ActorStatus::Running {
                return;
            }
            let _effects = actor.logic.receive(event, &mut actor.snapshot);
        }
    }

    fn stop(&self) {
        let mut actors = self.actors.lock().unwrap();
        if let Some(actor) = actors.get_mut(&self.id) {
            actor.logic.stop();
            actor.status = ActorStatus::Stopped;
        }
    }

    fn snapshot(&self) -> serde_json::Value {
        self.actors
            .lock()
            .unwrap()
            .get(&self.id)
            .map(|a| a.snapshot.clone())
            .unwrap_or(serde_json::Value::Null)
    }

    fn status(&self) -> ActorStatus {
        self.actors
            .lock()
            .unwrap()
            .get(&self.id)
            .map(|a| a.status)
            .unwrap_or(ActorStatus::Error)
    }
}

impl BackendActorSystem for NativeActorSystem {
    type Handle = NativeActorHandle;

    fn spawn(&self, id: &str, logic: Box<dyn ActorLogic>) -> Self::Handle {
        let snapshot = logic.initial_snapshot();
        let actor = ActorState {
            logic,
            snapshot,
            status: ActorStatus::Running,
        };
        self.actors.lock().unwrap().insert(id.to_string(), actor);
        NativeActorHandle {
            actors: Arc::clone(&self.actors),
            id: id.to_string(),
        }
    }

    fn send_to(&self, id: &str, event: serde_json::Value) {
        self.dispatch(id, event);
    }

    fn stop(&self, id: &str) {
        self.stop_actor(id);
    }

    fn snapshot(&self, id: &str) -> Option<serde_json::Value> {
        self.get_snapshot(id)
    }

    fn status(&self, id: &str) -> Option<ActorStatus> {
        self.get_status(id)
    }

    fn len(&self) -> usize {
        self.actors.lock().unwrap().len()
    }

    fn is_empty(&self) -> bool {
        self.actors.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xstate_core::actor_logic::ReducerLogic;

    #[test]
    fn spawn_and_send() {
        let system = NativeActorSystem::new();
        let logic = ReducerLogic::new(
            "counter",
            serde_json::json!({"count": 0}),
            |state, event| {
                let mut s = state.clone();
                if event["type"].as_str() == Some("INC") {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(c + 1));
                }
                (s, vec![])
            },
        );
        let handle = system.spawn("c1", Box::new(logic));
        assert_eq!(system.len(), 1);

        let snap = handle.snapshot();
        assert_eq!(snap["count"], 0);

        handle.send(serde_json::json!({"type": "INC"}));
        let snap = handle.snapshot();
        assert_eq!(snap["count"], 1);
    }

    #[test]
    fn system_dispatch_and_handle_dispatch_equivalent() {
        let system = NativeActorSystem::new();
        let logic = ReducerLogic::new(
            "counter",
            serde_json::json!({"count": 0}),
            |state, event| {
                let mut s = state.clone();
                if event["type"].as_str() == Some("INC") {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(c + 1));
                }
                (s, vec![])
            },
        );
        let handle = system.spawn("c1", Box::new(logic));

        // Use system dispatch
        system.send_to("c1", serde_json::json!({"type": "INC"}));
        assert_eq!(handle.snapshot()["count"], 1);

        // Use handle send
        handle.send(serde_json::json!({"type": "INC"}));
        assert_eq!(handle.snapshot()["count"], 2);
        assert_eq!(system.snapshot("c1").unwrap()["count"], 2);
    }

    #[test]
    fn stop_actor() {
        let system = NativeActorSystem::new();
        let logic = ReducerLogic::new("r", serde_json::json!({}), |s, _| (s.clone(), vec![]));
        let handle = system.spawn("a1", Box::new(logic));
        assert_eq!(handle.status(), ActorStatus::Running);

        handle.stop();
        assert_eq!(handle.status(), ActorStatus::Stopped);
    }

    #[test]
    fn unknown_actor_returns_defaults() {
        let system = NativeActorSystem::new();
        assert!(system.snapshot("nonexistent").is_none());
        assert!(system.status("nonexistent").is_none());
    }

    #[test]
    fn multiple_actors_independent() {
        let system = NativeActorSystem::new();
        let logic_a = ReducerLogic::new("a", serde_json::json!({"v": 1}), |state, event| {
            let mut s = state.clone();
            if event["type"].as_str() == Some("INC") {
                let v = s["v"].as_i64().unwrap_or(0);
                s.as_object_mut()
                    .unwrap()
                    .insert("v".into(), serde_json::json!(v + 1));
            }
            (s, vec![])
        });
        let logic_b = ReducerLogic::new("b", serde_json::json!({"v": 10}), |state, event| {
            let mut s = state.clone();
            if event["type"].as_str() == Some("INC") {
                let v = s["v"].as_i64().unwrap_or(0);
                s.as_object_mut()
                    .unwrap()
                    .insert("v".into(), serde_json::json!(v + 1));
            }
            (s, vec![])
        });

        let ha = system.spawn("a", Box::new(logic_a));
        let hb = system.spawn("b", Box::new(logic_b));

        ha.send(serde_json::json!({"type": "INC"}));
        ha.send(serde_json::json!({"type": "INC"}));

        assert_eq!(ha.snapshot()["v"], 3);
        assert_eq!(hb.snapshot()["v"], 10);
    }

    #[test]
    fn stopped_actor_ignores_events() {
        let system = NativeActorSystem::new();
        let logic = ReducerLogic::new("r", serde_json::json!({"v": 0}), |state, event| {
            let mut s = state.clone();
            if event["type"].as_str() == Some("INC") {
                let v = s["v"].as_i64().unwrap_or(0);
                s.as_object_mut()
                    .unwrap()
                    .insert("v".into(), serde_json::json!(v + 1));
            }
            (s, vec![])
        });
        let handle = system.spawn("r", Box::new(logic));
        handle.stop();
        handle.send(serde_json::json!({"type": "INC"}));
        assert_eq!(handle.snapshot()["v"], 0);
    }

    #[test]
    fn is_empty_len() {
        let system = NativeActorSystem::new();
        assert!(system.is_empty());
        assert_eq!(system.len(), 0);

        let logic = ReducerLogic::new("r", serde_json::json!({}), |s, _| (s.clone(), vec![]));
        system.spawn("a", Box::new(logic));
        assert!(!system.is_empty());
        assert_eq!(system.len(), 1);
    }

    #[test]
    fn machine_logic_guard_registry_blocks_and_fires_transition() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        use xstate_core::actions::ActionRegistry;
        use xstate_core::actor_logic::MachineLogic;
        use xstate_core::clock::MockClock;
        use xstate_core::guards::{GuardImpl, GuardRegistry};
        use xstate_core::interpreter::Interpreter;
        use xstate_core::xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

        let allowed = Arc::new(AtomicBool::new(false));
        let flag = allowed.clone();
        let mut guards = GuardRegistry::new();
        guards.register(GuardImpl::new("canToggle", move |_, _| {
            flag.load(Ordering::SeqCst)
        }));

        let machine = Machine {
            id: "guarded".into(),
            initial: "off".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                StateNode {
                    name: "off".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "TOGGLE".into(),
                        target: Some("on".into()),
                        guard: Some(xstate_core::xstate_ir::Guard {
                            name: "canToggle".into(),
                        }),
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
                StateNode {
                    name: "on".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
            ],
            setup: None,
        };

        let logic = MachineLogic::new_with_registries(
            Interpreter::new(machine),
            MockClock::new(0),
            guards,
            ActionRegistry::new(),
        );
        let system = NativeActorSystem::new();
        let handle = system.spawn("guarded", Box::new(logic));

        handle.send(serde_json::json!({"type": "TOGGLE"}));
        assert_eq!(handle.snapshot()["value"], "off");

        allowed.store(true, Ordering::SeqCst);
        handle.send(serde_json::json!({"type": "TOGGLE"}));
        assert_eq!(handle.snapshot()["value"], "on");
    }
}
