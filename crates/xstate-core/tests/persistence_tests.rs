use xstate_core::ActorLogic;
use xstate_core::Interpreter;
use xstate_core::MachineLogic;
use xstate_core::StateValue;
use xstate_core::clock::MockClock;
use xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

fn no_guards() -> impl Fn(&str, &serde_json::Value, &serde_json::Value) -> bool {
    |_: &str, _: &serde_json::Value, _: &serde_json::Value| true
}

fn no_actions() -> impl FnMut(&str, &[serde_json::Value], &serde_json::Value) {
    |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {}
}

fn atomic(name: &str, transitions: Vec<Transition>) -> StateNode {
    StateNode {
        name: name.into(),
        description: None,
        initial: None,
        entry: vec![],
        exit: vec![],
        transitions,
        children: vec![],
        node_type: StateType::Atomic,
        tags: vec![],
        invoke: None,
        on_done: vec![],
        output: None,
    }
}

fn trans(event: &str, target: &str) -> Transition {
    Transition {
        event: event.into(),
        target: Some(target.into()),
        guard: None,
        actions: vec![],
        description: None,
        delay_ms: None,
        targets: vec![],
    }
}

fn history_machine() -> Machine {
    Machine {
        id: "history".into(),
        initial: "main".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "main".into(),
                description: None,
                initial: Some("b1".into()),
                entry: vec![],
                exit: vec![],
                transitions: vec![trans("SWITCH", "other")],
                children: vec![
                    atomic("b1", vec![trans("NEXT", "b2")]),
                    atomic("b2", vec![]),
                    StateNode {
                        name: "hist".into(),
                        description: None,
                        initial: None,
                        entry: vec![],
                        exit: vec![],
                        transitions: vec![],
                        children: vec![],
                        node_type: StateType::History,
                        tags: vec![],
                        invoke: None,
                        on_done: vec![],
                        output: None,
                    },
                ],
                node_type: StateType::Compound,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
            },
            atomic("other", vec![trans("BACK", "hist")]),
        ],
        setup: None,
    }
}

fn after_machine() -> Machine {
    Machine {
        id: "timed".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            atomic("idle", vec![trans("START", "running")]),
            StateNode {
                name: "running".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "AFTER_100".into(),
                    target: Some("done".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: Some(100),
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
            },
            atomic("done", vec![]),
        ],
        setup: None,
    }
}

#[test]
fn persisted_snapshot_includes_history_entry_times_and_version() {
    let interpreter = Interpreter::new(history_machine());
    let state = interpreter.initial_state();
    let ctx = interpreter.initial_context();

    let persisted = interpreter.persisted_snapshot(&state, &ctx);

    assert_eq!(persisted["version"], 1);
    assert_eq!(persisted["value"]["main"], "b1");
    assert_eq!(persisted["context"], serde_json::json!({}));
    assert_eq!(persisted["history"]["main"], "b1");
    assert_eq!(persisted["entry_times"]["main"], 0);
}

#[test]
fn history_target_restores_to_recorded_child_not_initial() {
    let machine = history_machine();
    let interpreter = Interpreter::new(machine.clone());
    let mut state = interpreter.initial_state();
    let mut ctx = interpreter.initial_context();

    state = interpreter
        .send(
            &state,
            &serde_json::json!({"type": "NEXT"}),
            &mut ctx,
            &no_guards(),
            &mut no_actions(),
            &MockClock::new(0),
        )
        .unwrap();
    state = interpreter
        .send(
            &state,
            &serde_json::json!({"type": "SWITCH"}),
            &mut ctx,
            &no_guards(),
            &mut no_actions(),
            &MockClock::new(0),
        )
        .unwrap();
    assert_eq!(state, StateValue::Atomic("other".into()));

    let persisted = interpreter.persisted_snapshot(&state, &ctx);
    assert_eq!(persisted["history"]["main"], "b2");

    let fresh = Interpreter::new(machine);
    let (restored, restored_ctx) = fresh.restore_persisted(&persisted).unwrap();
    assert_eq!(restored, StateValue::Atomic("other".into()));
    assert_eq!(restored_ctx, ctx);

    let mut ctx2 = restored_ctx;
    let back = fresh
        .send(
            &restored,
            &serde_json::json!({"type": "BACK"}),
            &mut ctx2,
            &no_guards(),
            &mut no_actions(),
            &MockClock::new(0),
        )
        .unwrap();
    assert_eq!(
        back,
        StateValue::Compound {
            state: "main".into(),
            child: Box::new(StateValue::Atomic("b2".into())),
        },
        "history sibling target must return to the recorded child b2, not initial b1"
    );
}

#[test]
fn after_deadline_is_preserved_across_persist_restore() {
    let machine = after_machine();
    let interpreter = Interpreter::new(machine.clone());
    let state = interpreter.initial_state();
    let mut ctx = interpreter.initial_context();

    let entered_at = 1000;
    let state = interpreter
        .send(
            &state,
            &serde_json::json!({"type": "START"}),
            &mut ctx,
            &no_guards(),
            &mut no_actions(),
            &MockClock::new(entered_at),
        )
        .unwrap();
    assert_eq!(state, StateValue::Atomic("running".into()));

    let persisted = interpreter.persisted_snapshot(&state, &ctx);
    assert_eq!(persisted["entry_times"]["running"], entered_at);

    let deadline = entered_at + 100;

    let mut clock = MockClock::new(deadline - 1);
    let fresh = Interpreter::new(machine.clone());
    let (restored, mut ctx2) = fresh.restore_persisted(&persisted).unwrap();
    let result = fresh.send(
        &restored,
        &serde_json::json!({"type": "NOOP"}),
        &mut ctx2,
        &no_guards(),
        &mut no_actions(),
        &clock,
    );
    assert_eq!(
        result, None,
        "deadline must be preserved: one tick before entered_at + delay the after-transition must not fire"
    );

    clock.advance(1);
    let mut ctx3 = ctx2;
    let result = fresh.send(
        &restored,
        &serde_json::json!({"type": "NOOP"}),
        &mut ctx3,
        &no_guards(),
        &mut no_actions(),
        &clock,
    );
    assert_eq!(
        result,
        Some(StateValue::Atomic("done".into())),
        "next send at exactly entered_at + delay must fire the preserved after deadline"
    );
}

#[test]
fn old_format_value_context_still_restores() {
    let interpreter = Interpreter::new(after_machine());
    let old = serde_json::json!({
        "value": "idle",
        "context": {"count": 3},
    });

    let (state, ctx) = interpreter.restore_persisted(&old).unwrap();
    assert_eq!(state, StateValue::Atomic("idle".into()));
    assert_eq!(ctx, serde_json::json!({"count": 3}));
}

#[test]
fn unsupported_version_is_rejected() {
    let interpreter = Interpreter::new(after_machine());
    let future = serde_json::json!({
        "value": "idle",
        "context": {},
        "version": 999,
    });
    assert!(interpreter.restore_persisted(&future).is_err());
}

#[test]
fn machine_logic_persist_restore_pass_through() {
    let mut logic_a = MachineLogic::new(Interpreter::new(after_machine()), MockClock::new(1000));
    let mut snap = logic_a.initial_snapshot();
    logic_a.receive(serde_json::json!({"type": "START"}), &mut snap);
    assert_eq!(snap["value"], "running");

    let persisted = logic_a.persisted_snapshot(&snap);
    assert_eq!(persisted["version"], 1);
    assert_eq!(persisted["entry_times"]["running"], 1000);

    let mut logic_b = MachineLogic::new(Interpreter::new(after_machine()), MockClock::new(1050));
    let mut snap_b = logic_b.restore_persisted(&persisted).unwrap();
    assert_eq!(snap_b["value"], "running");
    logic_b.receive(serde_json::json!({"type": "NOOP"}), &mut snap_b);
    assert_eq!(
        snap_b["value"], "running",
        "deadline preserved: must not fire before entered_at + delay"
    );

    let mut logic_c = MachineLogic::new(Interpreter::new(after_machine()), MockClock::new(1100));
    let mut snap_c = logic_c.restore_persisted(&persisted).unwrap();
    logic_c.receive(serde_json::json!({"type": "NOOP"}), &mut snap_c);
    assert_eq!(
        snap_c["value"], "done",
        "deadline preserved: fires exactly at entered_at + delay"
    );
}

#[test]
fn machine_logic_restore_rejects_old_version() {
    let logic = MachineLogic::new(Interpreter::new(after_machine()), MockClock::new(0));
    let bad = serde_json::json!({"value": "idle", "context": {}, "version": 2});
    assert!(logic.restore_persisted(&bad).is_err());
}
