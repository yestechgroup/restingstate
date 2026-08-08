use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use xstate_core::clock::Clock;
use xstate_core::Interpreter;

use crate::actor::spawn_actor;
use crate::handle::{ActorHandle, ActorStatus};

/// Manages a collection of spawned actors, routing messages between them.
///
/// # Example
///
/// ```rust,ignore
/// let mut system = ActorSystem::new();
/// let actor = system.spawn(interpreter, MockClock::new(0), "loader".into());
/// system.send_to("loader", json!({"type": "FETCH"}));
/// let snap = system.snapshot("loader").unwrap();
/// ```
pub struct ActorSystem {
    actors: Arc<Mutex<HashMap<String, Box<dyn ActorHandle>>>>,
}

impl ActorSystem {
    pub fn new() -> Self {
        ActorSystem {
            actors: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn an actor from an interpreter. The actor starts in the interpreter's
    /// initial state. Returns the actor's ID.
    pub fn spawn(
        &self,
        interpreter: Interpreter,
        clock: impl Clock + Send + 'static,
        id: String,
    ) -> String {
        let handle = spawn_actor(interpreter, clock, String::new(), id.clone(), None, None);
        self.actors.lock().unwrap().insert(id.clone(), handle);
        id
    }

    /// Spawn an actor with a custom initial state and context.
    pub fn spawn_with_state(
        &self,
        interpreter: Interpreter,
        clock: impl Clock + Send + 'static,
        id: String,
        initial_state: xstate_core::StateValue,
        initial_context: serde_json::Value,
    ) -> String {
        let handle = spawn_actor(
            interpreter,
            clock,
            String::new(),
            id.clone(),
            Some(initial_state),
            Some(initial_context),
        );
        self.actors.lock().unwrap().insert(id.clone(), handle);
        id
    }

    /// Send an event to a named actor.
    pub fn send_to(&self, id: &str, event: serde_json::Value) {
        if let Some(actor) = self.actors.lock().unwrap().get(id) {
            actor.send(event);
        }
    }

    /// Stop a named actor.
    pub fn stop(&self, id: &str) {
        if let Some(actor) = self.actors.lock().unwrap().get(id) {
            actor.stop();
        }
    }

    /// Get a snapshot of a named actor's current state.
    pub fn snapshot(&self, id: &str) -> Option<serde_json::Value> {
        self.actors.lock().unwrap().get(id).map(|a| a.snapshot())
    }

    /// Get an actor's status.
    pub fn status(&self, id: &str) -> Option<ActorStatus> {
        self.actors.lock().unwrap().get(id).map(|a| a.status())
    }

    /// Send an event to all actors.
    pub fn broadcast(&self, event: serde_json::Value) {
        for actor in self.actors.lock().unwrap().values() {
            actor.send(event.clone());
        }
    }

    /// Stop all actors.
    pub fn stop_all(&self) {
        for actor in self.actors.lock().unwrap().values() {
            actor.stop();
        }
    }

    /// Remove stopped/done actors from the registry.
    pub fn gc(&self) {
        self.actors.lock().unwrap().retain(|_, a| {
            let s = a.status();
            s == ActorStatus::Running
        });
    }

    /// Number of registered actors.
    pub fn len(&self) -> usize {
        self.actors.lock().unwrap().len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.actors.lock().unwrap().is_empty()
    }
}

impl Default for ActorSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xstate_core::clock::MockClock;
    use xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

    fn toggle_machine() -> Machine {
        Machine {
            id: "toggle".into(),
            initial: "inactive".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                StateNode {
                    name: "inactive".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "TOGGLE".into(),
                        target: Some("active".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                    invoke: None,
                },
                StateNode {
                    name: "active".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "TOGGLE".into(),
                        target: Some("inactive".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                    invoke: None,
                },
            ],
        }
    }

    #[tokio::test]
    async fn spawn_and_send_event() {
        let system = ActorSystem::new();
        let interpreter = Interpreter::new(toggle_machine());

        let id = system.spawn(interpreter, MockClock::new(0), "toggle".into());
        assert_eq!(system.len(), 1);

        let snap = system.snapshot(&id).unwrap();
        assert_eq!(snap["value"], serde_json::json!("inactive"));

        system.send_to(&id, serde_json::json!({"type": "TOGGLE"}));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let snap = system.snapshot(&id).unwrap();
        assert_eq!(snap["value"], serde_json::json!("active"));
    }

    #[tokio::test]
    async fn stop_actor() {
        let system = ActorSystem::new();
        let interpreter = Interpreter::new(toggle_machine());

        let id = system.spawn(interpreter, MockClock::new(0), "t".into());
        assert_eq!(system.len(), 1);

        system.stop(&id);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(system.status(&id), Some(ActorStatus::Stopped));
    }

    #[tokio::test]
    async fn gc_removes_stopped() {
        let system = ActorSystem::new();
        let interpreter = Interpreter::new(toggle_machine());

        let id = system.spawn(interpreter, MockClock::new(0), "t".into());
        system.stop(&id);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        system.gc();

        assert!(system.is_empty());
    }

    #[tokio::test]
    async fn unknown_event_does_not_transition() {
        let system = ActorSystem::new();
        let interpreter = Interpreter::new(toggle_machine());

        let id = system.spawn(interpreter, MockClock::new(0), "t".into());

        system.send_to(&id, serde_json::json!({"type": "UNKNOWN"}));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let snap = system.snapshot(&id).unwrap();
        assert_eq!(snap["value"], serde_json::json!("inactive"));
    }
}
