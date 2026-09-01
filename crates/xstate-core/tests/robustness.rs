// Interpreter send() robustness matrix — issue #29.
use xstate_core::StateValue;
use xstate_core::clock::MockClock;
use xstate_core::interpreter::{Interpreter, PersistError};
use xstate_ir::{Action, ContextSchema, Guard, Machine, StateNode, StateType, Transition};

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

fn toggle() -> Machine {
    Machine {
        id: "toggle".into(),
        initial: "inactive".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            atomic("inactive", vec![trans("toggle", "active")]),
            atomic("active", vec![trans("toggle", "inactive")]),
        ],
        setup: None,
    }
}

fn send(
    interpreter: &Interpreter,
    state: &StateValue,
    event: &serde_json::Value,
) -> Option<StateValue> {
    let mut context = interpreter.initial_context();
    interpreter.send(
        state,
        event,
        &mut context,
        &no_guards(),
        &mut no_actions(),
        &MockClock::new(0),
    )
}

#[test]
fn null_event_does_not_panic() {
    let interpreter = Interpreter::new(toggle());
    let state = interpreter.initial_state();
    assert!(send(&interpreter, &state, &serde_json::Value::Null).is_none());
}

#[test]
fn non_string_type_field_does_not_panic() {
    let interpreter = Interpreter::new(toggle());
    let state = interpreter.initial_state();
    assert!(send(&interpreter, &state, &serde_json::json!({"type": 123})).is_none());
    assert!(
        send(
            &interpreter,
            &state,
            &serde_json::json!({"type": {"deep": []}})
        )
        .is_none()
    );
}

#[test]
fn array_event_does_not_panic() {
    let interpreter = Interpreter::new(toggle());
    let state = interpreter.initial_state();
    assert!(send(&interpreter, &state, &serde_json::json!([])).is_none());
    assert!(send(&interpreter, &state, &serde_json::json!([1, 2, 3])).is_none());
}

#[test]
fn one_megabyte_payload_is_bounded_work() {
    let interpreter = Interpreter::new(toggle());
    let state = interpreter.initial_state();
    let event = serde_json::json!({ "type": "toggle", "payload": "x".repeat(1024 * 1024) });
    let next = send(&interpreter, &state, &event);
    assert_eq!(next, Some(StateValue::Atomic("active".into())));
}

#[test]
fn deeply_nested_event_object_does_not_panic_or_hang() {
    // The interpreter only reads event["type"], so it survives arbitrary
    // nesting; the scenario runs on a large-stack thread because dropping a
    // 5k-deep serde_json::Value is itself recursive (#29).
    let result = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let interpreter = Interpreter::new(toggle());
            let state = interpreter.initial_state();
            let mut event = serde_json::json!({ "type": "toggle" });
            for _ in 0..5_000 {
                event = serde_json::json!({ "wrap": event });
            }
            send(&interpreter, &state, &event)
        })
        .expect("spawn robustness thread")
        .join()
        .expect("send must not panic on a deeply nested event");
    assert!(result.is_none(), "a wrapped event cannot drive transitions");
}

#[test]
fn eventless_machine_with_null_event_does_not_panic() {
    // A null event maps to type "" which can match eventless transitions;
    // whatever the outcome it must not panic (#29).
    let machine = Machine {
        id: "always".into(),
        initial: "start".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            atomic(
                "start",
                vec![Transition {
                    event: String::new(),
                    target: Some("done".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
            ),
            atomic("done", vec![]),
        ],
        setup: None,
    };
    let interpreter = Interpreter::new(machine);
    let state = interpreter.initial_state();
    let next = send(&interpreter, &state, &serde_json::Value::Null);
    assert!(next.is_some() || next.is_none());
}

#[test]
fn unknown_initial_state_is_reported_not_panicking() {
    let mut machine = toggle();
    machine.initial = "ghost".into();
    let interpreter = Interpreter::new(machine);

    assert!(Interpreter::validate_machine(interpreter.machine()).is_err());

    // Safe fallback: the interpreter stays usable and never panics (#29).
    let state = interpreter.initial_state();
    assert!(send(&interpreter, &state, &serde_json::json!({"type": "toggle"})).is_none());
}

#[test]
fn compound_initial_child_missing_is_reported_not_panicking() {
    let mut broken = atomic("broken", vec![]);
    broken.node_type = StateType::Compound;
    broken.initial = Some("ghost".into());
    broken.children = vec![atomic("real", vec![])];

    let machine = Machine {
        id: "m".into(),
        initial: "broken".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![broken, atomic("idle", vec![])],
        setup: None,
    };
    let interpreter = Interpreter::new(machine);

    let err = Interpreter::validate_machine(interpreter.machine())
        .expect_err("missing initial child must be reported");
    assert!(err.to_string().contains("ghost"), "{err}");

    // Safe fallback: usable interpreter, no panic (#29).
    let state = interpreter.initial_state();
    let _ = send(&interpreter, &state, &serde_json::json!({"type": "GO"}));
}

#[test]
fn transition_into_broken_compound_does_not_panic() {
    let mut broken = atomic("broken", vec![]);
    broken.node_type = StateType::Compound;
    broken.initial = Some("ghost".into());
    broken.children = vec![atomic("real", vec![])];

    let machine = Machine {
        id: "m".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![atomic("idle", vec![trans("GO", "broken")]), broken],
        setup: None,
    };
    let interpreter = Interpreter::new(machine);
    assert!(Interpreter::validate_machine(interpreter.machine()).is_err());

    let state = interpreter.initial_state();
    let next = send(&interpreter, &state, &serde_json::json!({"type": "GO"}));
    assert!(
        next.is_some(),
        "transition must land somewhere safe, not panic"
    );
}

#[test]
fn valid_machine_passes_validation() {
    let interpreter = Interpreter::new(toggle());
    assert!(Interpreter::validate_machine(interpreter.machine()).is_ok());
}

#[test]
fn target_of_nonexistent_state_returns_none_not_panic() {
    let machine = Machine {
        id: "m".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![atomic("idle", vec![trans("GO", "nowhere")])],
        setup: None,
    };
    let interpreter = Interpreter::new(machine);
    let state = interpreter.initial_state();
    assert!(send(&interpreter, &state, &serde_json::json!({"type": "GO"})).is_none());
}

#[test]
fn delay_u64_max_never_overflows_arithmetic() {
    // Entering "waiting" at t=5 makes resolve_after compute 5 + u64::MAX,
    // which overflowed and panicked before the saturating fix (#29). The
    // "done" target deliberately does not exist so resolve_always cannot
    // resolve the transition and the delay arithmetic is really exercised.
    let waiting = atomic(
        "waiting",
        vec![Transition {
            event: String::new(),
            target: Some("done".into()),
            guard: None,
            actions: vec![],
            description: None,
            delay_ms: Some(u64::MAX),
            targets: vec![],
        }],
    );
    let machine = Machine {
        id: "m".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![atomic("idle", vec![trans("GO", "waiting")]), waiting],
        setup: None,
    };

    let interpreter = Interpreter::new(machine);
    let state = interpreter.initial_state();
    let next = send_with_clock(&interpreter, &state, &serde_json::json!({"type": "GO"}), 5);
    assert_eq!(next, Some(StateValue::Atomic("waiting".into())));
}

#[test]
fn delayed_transition_fires_once_clock_passes_the_delay() {
    let waiting = atomic(
        "waiting",
        vec![Transition {
            event: String::new(),
            target: Some("done".into()),
            guard: None,
            actions: vec![],
            description: None,
            delay_ms: Some(50),
            targets: vec![],
        }],
    );
    let machine = Machine {
        id: "m".into(),
        initial: "waiting".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![waiting, atomic("done", vec![])],
        setup: None,
    };

    let interpreter = Interpreter::new(machine);
    let state = interpreter.initial_state();
    assert_eq!(
        send_with_clock(
            &interpreter,
            &state,
            &serde_json::json!({"type": "NOOP"}),
            10
        ),
        None,
        "the delay must gate the transition"
    );
    assert_eq!(
        send_with_clock(
            &interpreter,
            &state,
            &serde_json::json!({"type": "NOOP"}),
            50
        ),
        Some(StateValue::Atomic("done".into()))
    );
}

fn send_with_clock(
    interpreter: &Interpreter,
    state: &StateValue,
    event: &serde_json::Value,
    now: u64,
) -> Option<StateValue> {
    let mut context = interpreter.initial_context();
    interpreter.send(
        state,
        event,
        &mut context,
        &no_guards(),
        &mut no_actions(),
        &MockClock::new(now),
    )
}

#[test]
fn malformed_assign_ops_do_not_panic() {
    let assign_action = |args: serde_json::Value| Transition {
        event: "GO".into(),
        target: None,
        guard: None,
        actions: vec![Action {
            name: "assign".into(),
            args: vec![args],
        }],
        description: None,
        delay_ms: None,
        targets: vec![],
    };

    let context = ContextSchema {
        fields: vec![xstate_ir::ContextField {
            name: "count".into(),
            type_name: "number".into(),
            default_value: serde_json::json!(0),
        }],
    };

    for ops in [
        serde_json::json!({ "$assign_ops": "not-an-array" }),
        serde_json::json!({ "$assign_ops": [{ "op": "teleport", "field": "count" }] }),
        serde_json::json!({ "$assign_ops": [{ "op": "set", "field": "count", "from": 42 }] }),
        serde_json::json!({ "$assign_ops": [{ "op": "set", "field": "count", "from": "nodot" }] }),
        serde_json::json!({ "$assign_ops": [{ "op": "set", "field": "count", "from": "event.nope" }] }),
        serde_json::json!({ "$assign_ops": [{ "op": "add", "field": "count" }] }),
        serde_json::json!({ "$assign_ops": [{ "op": "add", "field": "count", "value": "NaN" }] }),
    ] {
        let machine = Machine {
            id: "m".into(),
            initial: "idle".into(),
            context: context.clone(),
            states: vec![atomic("idle", vec![assign_action(ops)])],
            setup: None,
        };
        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        let next = send(&interpreter, &state, &serde_json::json!({"type": "GO"}));
        assert!(next.is_some(), "internal transitions must still complete");
    }
}

#[test]
fn restore_persisted_rejects_malformed_snapshots_with_typed_errors() {
    let interpreter = Interpreter::new(toggle());

    let wrong_version = serde_json::json!({"value": "inactive", "context": {}, "version": 99});
    assert!(matches!(
        interpreter.restore_persisted(&wrong_version),
        Err(PersistError::UnsupportedVersion(99))
    ));

    let non_numeric_version =
        serde_json::json!({"value": "inactive", "context": {}, "version": "one"});
    assert!(matches!(
        interpreter.restore_persisted(&non_numeric_version),
        Err(PersistError::UnsupportedVersion(0))
    ));

    let missing_value = serde_json::json!({"context": {}, "version": 1});
    assert!(matches!(
        interpreter.restore_persisted(&missing_value),
        Err(PersistError::MissingValue)
    ));

    let wrong_value_type = serde_json::json!({"value": 42, "context": {}, "version": 1});
    assert!(matches!(
        interpreter.restore_persisted(&wrong_value_type),
        Err(PersistError::InvalidStateValue(_))
    ));

    let unknown_state = serde_json::json!({"value": "limbo", "context": {}, "version": 1});
    assert!(matches!(
        interpreter.restore_persisted(&unknown_state),
        Err(PersistError::InvalidStateValue(_))
    ));

    let garbage_history =
        serde_json::json!({"value": "inactive", "context": {}, "version": 1, "history": 7});
    assert!(matches!(
        interpreter.restore_persisted(&garbage_history),
        Err(PersistError::InvalidHistory(_))
    ));

    let history_unknown_state = serde_json::json!({
        "value": "inactive", "context": {}, "version": 1,
        "history": {"parent": "limbo"}
    });
    assert!(matches!(
        interpreter.restore_persisted(&history_unknown_state),
        Err(PersistError::HistoryTargetInvalid(_))
    ));

    let garbage_entry_times =
        serde_json::json!({"value": "inactive", "context": {}, "version": 1, "entry_times": []});
    assert!(matches!(
        interpreter.restore_persisted(&garbage_entry_times),
        Err(PersistError::InvalidEntryTimes(_))
    ));
}

#[test]
fn guarded_send_with_malformed_guard_names_does_not_panic() {
    let mut guarded = trans("GO", "idle");
    guarded.guard = Some(Guard {
        name: String::new(),
    });
    let machine = Machine {
        id: "m".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![atomic("idle", vec![guarded])],
        setup: None,
    };
    let interpreter = Interpreter::new(machine);
    let state = interpreter.initial_state();
    let _ = send(&interpreter, &state, &serde_json::json!({"type": "GO"}));
}
