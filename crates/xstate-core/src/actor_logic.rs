use crate::clock::Clock;
use crate::interpreter::Interpreter;
use crate::state_value::StateValue;
use std::sync::Mutex;

/// Reducer function signature: `(state, event) -> (new_state, effects)`.
type ReducerFn = dyn Fn(&serde_json::Value, &serde_json::Value) -> (serde_json::Value, Vec<serde_json::Value>)
    + Send
    + Sync;

/// Callback function signature: `(event, sender)`.
type CallbackFn = dyn Fn(serde_json::Value, Sender) + Send + Sync;

/// Promise future type.
type PromiseFuture =
    dyn std::future::Future<Output = Result<serde_json::Value, serde_json::Value>> + Send + Sync;

/// Structured output from an actor's receive() method.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ActorOutput {
    /// No output.
    None,
    /// A raised event (internal to the actor).
    RaisedEvent(serde_json::Value),
    /// The actor's snapshot changed (for xstate.snapshot.* events).
    SnapshotChanged(serde_json::Value),
    /// A child actor should be spawned.
    ChildSpawned { id: String, logic_name: String },
    /// The actor is done (reached final state).
    Done(Option<serde_json::Value>),
}

/// Core trait defining how an actor processes events.
pub trait ActorLogic: Send + Sync {
    /// Human-readable name for debugging.
    fn name(&self) -> &str;

    /// Create the initial snapshot (serializable state).
    fn initial_snapshot(&self) -> serde_json::Value;

    /// Process an event and return any raised/output events.
    fn receive(
        &mut self,
        event: serde_json::Value,
        snapshot: &mut serde_json::Value,
    ) -> Vec<serde_json::Value>;

    /// Called when the actor is stopped. Returns optional final output.
    fn stop(&mut self) -> Option<serde_json::Value> {
        None
    }
}

/// Wraps an XState Interpreter as an ActorLogic.
pub struct MachineLogic {
    interpreter: Mutex<Interpreter>,
    machine_name: String,
    clock: Mutex<Box<dyn Clock>>,
}

impl MachineLogic {
    pub fn new(interpreter: Interpreter, clock: impl Clock + 'static) -> Self {
        let machine_name = interpreter.machine().id.clone();
        MachineLogic {
            interpreter: Mutex::new(interpreter),
            machine_name,
            clock: Mutex::new(Box::new(clock)),
        }
    }
}

impl ActorLogic for MachineLogic {
    fn name(&self) -> &str {
        &self.machine_name
    }

    fn initial_snapshot(&self) -> serde_json::Value {
        let interp = self.interpreter.lock().unwrap();
        let state = interp.initial_state();
        let context = interp.initial_context();
        serde_json::json!({
            "value": serde_json::to_value(&state).unwrap(),
            "context": context,
        })
    }

    fn receive(
        &mut self,
        event: serde_json::Value,
        snapshot: &mut serde_json::Value,
    ) -> Vec<serde_json::Value> {
        let interp = self.interpreter.lock().unwrap();
        let state: StateValue = serde_json::from_value(snapshot["value"].clone())
            .unwrap_or_else(|_| interp.initial_state());
        let mut context = snapshot
            .get("context")
            .cloned()
            .unwrap_or_else(|| interp.initial_context());

        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

        let clock = self.clock.lock().unwrap();
        let new_state = interp.send(
            &state,
            &event,
            &mut context,
            &guards,
            &mut actions,
            clock.as_ref(),
        );
        drop(clock);
        drop(interp);

        if let Some(s) = new_state {
            *snapshot = serde_json::json!({
                "value": serde_json::to_value(&s).unwrap(),
                "context": context,
            });
        }

        vec![]
    }
}

/// Pure reducer pattern actor logic.
pub struct ReducerLogic {
    name: String,
    initial: serde_json::Value,
    reducer: Box<ReducerFn>,
}

impl ReducerLogic {
    pub fn new(
        name: impl Into<String>,
        initial: serde_json::Value,
        reducer: impl Fn(
            &serde_json::Value,
            &serde_json::Value,
        ) -> (serde_json::Value, Vec<serde_json::Value>)
        + Send
        + Sync
        + 'static,
    ) -> Self {
        ReducerLogic {
            name: name.into(),
            initial,
            reducer: Box::new(reducer),
        }
    }
}

impl ActorLogic for ReducerLogic {
    fn name(&self) -> &str {
        &self.name
    }

    fn initial_snapshot(&self) -> serde_json::Value {
        self.initial.clone()
    }

    fn receive(
        &mut self,
        event: serde_json::Value,
        snapshot: &mut serde_json::Value,
    ) -> Vec<serde_json::Value> {
        let (new_state, effects) = (self.reducer)(snapshot, &event);
        *snapshot = new_state;
        effects
    }
}

/// Callback actor logic — a long-running callback that receives a sender to
/// send events back.
pub struct CallbackLogic {
    name: String,
    initial: serde_json::Value,
    callback: Box<CallbackFn>,
}

impl CallbackLogic {
    pub fn new(
        name: impl Into<String>,
        initial: serde_json::Value,
        callback: impl Fn(serde_json::Value, Sender) + Send + Sync + 'static,
    ) -> Self {
        CallbackLogic {
            name: name.into(),
            initial,
            callback: Box::new(callback),
        }
    }
}

/// Sender for callback actors to send events back to the actor system.
pub struct Sender {
    tx: tokio::sync::mpsc::UnboundedSender<serde_json::Value>,
}

impl Sender {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<serde_json::Value>) -> Self {
        Sender { tx }
    }

    pub fn send(&self, event: serde_json::Value) {
        let _ = self.tx.send(event);
    }
}

impl ActorLogic for CallbackLogic {
    fn name(&self) -> &str {
        &self.name
    }

    fn initial_snapshot(&self) -> serde_json::Value {
        self.initial.clone()
    }

    fn receive(
        &mut self,
        event: serde_json::Value,
        _snapshot: &mut serde_json::Value,
    ) -> Vec<serde_json::Value> {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = Sender::new(tx);
        (self.callback)(event, sender);
        vec![]
    }
}

/// One-shot async promise actor logic.
pub struct PromiseLogic {
    name: String,
    resolved: Option<serde_json::Value>,
    pending: Option<std::pin::Pin<Box<PromiseFuture>>>,
}

impl PromiseLogic {
    pub fn new(
        name: impl Into<String>,
        promise: impl std::future::Future<Output = Result<serde_json::Value, serde_json::Value>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        PromiseLogic {
            name: name.into(),
            resolved: None,
            pending: Some(Box::pin(promise)),
        }
    }
}

impl ActorLogic for PromiseLogic {
    fn name(&self) -> &str {
        &self.name
    }

    fn initial_snapshot(&self) -> serde_json::Value {
        serde_json::json!({ "status": "pending" })
    }

    fn receive(
        &mut self,
        _event: serde_json::Value,
        snapshot: &mut serde_json::Value,
    ) -> Vec<serde_json::Value> {
        if self.resolved.is_some() {
            return vec![];
        }

        if let Some(future) = self.pending.as_mut() {
            let waker = std::task::Waker::noop();
            let mut cx = std::task::Context::from_waker(waker);
            match future.as_mut().poll(&mut cx) {
                std::task::Poll::Ready(result) => {
                    self.pending = None;
                    match result {
                        Ok(value) => {
                            self.resolved = Some(value.clone());
                            *snapshot = serde_json::json!({
                                "status": "resolved",
                                "value": value,
                            });
                        }
                        Err(err) => {
                            self.resolved = Some(err.clone());
                            *snapshot = serde_json::json!({
                                "status": "rejected",
                                "error": err,
                            });
                        }
                    }
                }
                std::task::Poll::Pending => {
                    *snapshot = serde_json::json!({ "status": "pending" });
                }
            }
        }

        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reducer_logic_basic() {
        let mut logic = ReducerLogic::new(
            "counter",
            serde_json::json!({"count": 0}),
            |state, event| {
                let mut new_state = state.clone();
                if event["type"].as_str() == Some("INC") {
                    let count = new_state["count"].as_i64().unwrap_or(0);
                    new_state
                        .as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(count + 1));
                }
                (new_state, vec![])
            },
        );

        assert_eq!(logic.name(), "counter");
        let snap = logic.initial_snapshot();
        assert_eq!(snap["count"], 0);

        let mut snap = snap;
        let events = logic.receive(serde_json::json!({"type": "INC"}), &mut snap);
        assert!(events.is_empty());
        assert_eq!(snap["count"], 1);
    }

    #[test]
    fn reducer_logic_returns_effects() {
        let mut logic = ReducerLogic::new("with-effects", serde_json::json!({}), |state, event| {
            if event["type"].as_str() == Some("PING") {
                (state.clone(), vec![serde_json::json!({"type": "PONG"})])
            } else {
                (state.clone(), vec![])
            }
        });

        let mut snap = logic.initial_snapshot();
        let events = logic.receive(serde_json::json!({"type": "PING"}), &mut snap);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "PONG");
    }

    #[test]
    fn promise_logic_resolves() {
        let mut logic = PromiseLogic::new("promise", async { Ok(serde_json::json!(42)) });
        assert_eq!(logic.name(), "promise");

        let mut snap = logic.initial_snapshot();
        assert_eq!(snap["status"], "pending");

        // The ready future resolves immediately on first poll
        logic.receive(serde_json::json!({"type": "CHECK"}), &mut snap);
        assert_eq!(snap["status"], "resolved");
        assert_eq!(snap["value"], 42);
    }

    #[test]
    fn machine_logic_name() {
        use xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

        let machine = Machine {
            id: "test_machine".into(),
            initial: "idle".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![StateNode {
                name: "idle".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "GO".into(),
                    target: Some("done".into()),
                    guard: None,
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
            }],

            setup: None,
        };
        let interp = Interpreter::new(machine);
        let logic = MachineLogic::new(interp, crate::clock::MockClock::new(0));
        assert_eq!(logic.name(), "test_machine");
    }

    #[test]
    fn machine_logic_initial_snapshot() {
        use xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

        let machine = Machine {
            id: "snap_test".into(),
            initial: "idle".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![StateNode {
                name: "idle".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "GO".into(),
                    target: Some("active".into()),
                    guard: None,
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
            }],

            setup: None,
        };
        let interp = Interpreter::new(machine);
        let logic = MachineLogic::new(interp, crate::clock::MockClock::new(0));
        let snap = logic.initial_snapshot();
        assert_eq!(snap["value"], "idle");
    }

    #[test]
    fn machine_logic_receive_transitions() {
        use xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

        let machine = Machine {
            id: "t".into(),
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
                        guard: None,
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
                    transitions: vec![Transition {
                        event: "TOGGLE".into(),
                        target: Some("off".into()),
                        guard: None,
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
            ],

            setup: None,
        };
        let interp = Interpreter::new(machine);
        let mut logic = MachineLogic::new(interp, crate::clock::MockClock::new(0));

        let mut snap = logic.initial_snapshot();
        assert_eq!(snap["value"], "off");

        logic.receive(serde_json::json!({"type": "TOGGLE"}), &mut snap);
        assert_eq!(snap["value"], "on");

        logic.receive(serde_json::json!({"type": "TOGGLE"}), &mut snap);
        assert_eq!(snap["value"], "off");
    }
}
