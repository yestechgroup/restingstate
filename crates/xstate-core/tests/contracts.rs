//! Pins the dependency-contract conventions documented in docs/contracts.md.

use std::sync::{Arc, Mutex};

use xstate_core::ActionRegistry;
use xstate_core::ActorLogic;
use xstate_core::GuardRegistry;
use xstate_core::Interpreter;
use xstate_core::MachineLogic;
use xstate_core::StateValue;
use xstate_core::clock::MockClock;
use xstate_core::interpreter::{PERSISTED_SNAPSHOT_VERSION, PersistError};
use xstate_ir::{Action, ContextField, ContextSchema, Machine, StateNode, StateType, Transition};

fn no_guards() -> impl Fn(&str, &serde_json::Value, &serde_json::Value) -> bool {
    |_: &str, _: &serde_json::Value, _: &serde_json::Value| true
}

fn no_actions() -> impl FnMut(&str, &[serde_json::Value], &serde_json::Value) {
    |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {}
}

fn field(name: &str, default_value: serde_json::Value) -> ContextField {
    ContextField {
        name: name.into(),
        type_name: "json".into(),
        default_value,
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

fn action_trans(event: &str, actions: Vec<Action>) -> Transition {
    Transition {
        event: event.into(),
        target: None,
        guard: None,
        actions,
        description: None,
        delay_ms: None,
        targets: vec![],
    }
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

fn machine(id: &str, initial: &str, fields: Vec<ContextField>, states: Vec<StateNode>) -> Machine {
    Machine {
        id: id.into(),
        initial: initial.into(),
        context: ContextSchema { fields },
        states,
        setup: None,
    }
}

fn toggle_machine() -> Machine {
    machine(
        "toggle",
        "inactive",
        vec![],
        vec![
            atomic("inactive", vec![trans("TOGGLE", "active")]),
            atomic("active", vec![trans("TOGGLE", "inactive")]),
        ],
    )
}

#[test]
fn persisted_snapshot_envelope_is_pinned() {
    let interp = Interpreter::new(toggle_machine());
    let state = interp.initial_state();
    let context = interp.initial_context();

    let persisted = interp.persisted_snapshot(&state, &context);

    assert_eq!(PERSISTED_SNAPSHOT_VERSION, 1);
    let mut keys: Vec<&str> = persisted
        .as_object()
        .expect("persisted snapshot must be a JSON object")
        .keys()
        .map(|k| k.as_str())
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["context", "entry_times", "history", "value", "version"]
    );
    assert_eq!(persisted["version"], 1);
    assert_eq!(persisted["value"], serde_json::json!("inactive"));
    assert_eq!(persisted["context"], context);
    assert_eq!(persisted["history"], serde_json::json!({}));
    assert_eq!(persisted["entry_times"], serde_json::json!({"inactive": 0}));
}

#[test]
fn legacy_value_context_snapshot_restores() {
    let interp = Interpreter::new(toggle_machine());

    let (state, context) = interp
        .restore_persisted(&serde_json::json!({"value": "active", "context": {"count": 7}}))
        .expect("a legacy {value, context} snapshot must restore");
    assert_eq!(state, StateValue::Atomic("active".into()));
    assert_eq!(context, serde_json::json!({"count": 7}));

    let persisted = interp.persisted_snapshot(&state, &context);
    assert_eq!(persisted["version"], 1);
    assert_eq!(persisted["value"], serde_json::json!("active"));
    assert_eq!(persisted["history"], serde_json::json!({}));
}

#[test]
fn restore_rejects_unknown_version_and_unknown_states() {
    let interp = Interpreter::new(toggle_machine());

    let err = interp
        .restore_persisted(&serde_json::json!({
            "value": "active",
            "context": {},
            "version": 2
        }))
        .unwrap_err();
    assert_eq!(err, PersistError::UnsupportedVersion(2));

    let err = interp
        .restore_persisted(&serde_json::json!({"value": "nope", "context": {}}))
        .unwrap_err();
    assert!(matches!(err, PersistError::InvalidStateValue(_)));
}

#[test]
fn plain_snapshot_shape_is_pinned() {
    let interp = Interpreter::new(toggle_machine());
    let state = interp.initial_state();
    let context = interp.initial_context();

    assert_eq!(
        interp.snapshot(&state, &context),
        serde_json::json!({"value": "inactive", "context": {}})
    );
}

fn assign_ops_machine() -> Machine {
    machine(
        "ops",
        "idle",
        vec![
            field("count", serde_json::json!(0)),
            field("last", serde_json::json!("")),
        ],
        vec![atomic(
            "idle",
            vec![action_trans(
                "BUMP",
                vec![Action {
                    name: "assign".into(),
                    args: vec![serde_json::json!({
                        "$assign_ops": [
                            {"op": "add", "field": "count", "value": 2},
                            {"op": "sub", "field": "count", "value": 1},
                            {"op": "set", "field": "last", "from": "event.label", "default": "none"},
                            {"op": "set", "field": "nested.deep", "value": "x"}
                        ]
                    })],
                }],
            )],
        )],
    )
}

#[test]
fn assign_ops_envelope_in_first_arg_is_applied() {
    let interp = Interpreter::new(assign_ops_machine());
    let state = interp.initial_state();
    let mut context = interp.initial_context();

    interp.send(
        &state,
        &serde_json::json!({"type": "BUMP", "label": "hi"}),
        &mut context,
        &no_guards(),
        &mut no_actions(),
        &MockClock::new(0),
    );
    assert_eq!(context["count"], 1);
    assert_eq!(context["last"], "hi");
    assert_eq!(context["nested"]["deep"], "x");

    interp.send(
        &state,
        &serde_json::json!({"type": "BUMP"}),
        &mut context,
        &no_guards(),
        &mut no_actions(),
        &MockClock::new(0),
    );
    assert_eq!(context["count"], 2);
    assert_eq!(context["last"], "none");
}

#[test]
fn assign_pairwise_literal_fallback_is_pinned() {
    let interp = machine_with_assign_args(
        "pairwise",
        vec![serde_json::json!("count"), serde_json::json!(5)],
    );
    let state = interp.initial_state();
    let mut context = interp.initial_context();

    interp.send(
        &state,
        &serde_json::json!({"type": "BUMP"}),
        &mut context,
        &no_guards(),
        &mut no_actions(),
        &MockClock::new(0),
    );
    assert_eq!(context["count"], 5);
}

fn machine_with_assign_args(id: &str, args: Vec<serde_json::Value>) -> Interpreter {
    let m = machine(
        id,
        "idle",
        vec![field("count", serde_json::json!(0))],
        vec![atomic(
            "idle",
            vec![action_trans(
                "BUMP",
                vec![Action {
                    name: "assign".into(),
                    args,
                }],
            )],
        )],
    );
    Interpreter::new(m)
}

#[test]
fn machine_logic_persisted_round_trip_shape_is_pinned() {
    let logic = MachineLogic::new(Interpreter::new(toggle_machine()), MockClock::new(0));

    let running = logic.initial_snapshot();
    assert_eq!(
        running,
        serde_json::json!({"value": "inactive", "context": {}})
    );

    let persisted = logic.persisted_snapshot(&running);
    assert_eq!(persisted["version"], 1);
    assert_eq!(persisted["value"], serde_json::json!("inactive"));

    let restored = logic.restore_persisted(&persisted).unwrap();
    assert_eq!(
        restored,
        serde_json::json!({"value": "inactive", "context": {}})
    );
}

type GuardCall = (String, serde_json::Value, serde_json::Value);
type ActionCall = (String, Vec<serde_json::Value>, serde_json::Value);

#[test]
fn guard_and_action_closure_argument_positions_are_pinned() {
    let guard_calls: Arc<Mutex<Vec<GuardCall>>> = Arc::new(Mutex::new(Vec::new()));
    let action_calls: Arc<Mutex<Vec<ActionCall>>> = Arc::new(Mutex::new(Vec::new()));

    let idle = {
        let mut t = trans("SUBMIT", "done");
        t.guard = Some(xstate_ir::Guard {
            name: "canSubmit".into(),
        });
        t.actions = vec![Action {
            name: "log".into(),
            args: vec![serde_json::json!("extra")],
        }];
        atomic("idle", vec![t])
    };
    let interp = Interpreter::new(machine(
        "forms",
        "idle",
        vec![],
        vec![idle, atomic("done", vec![])],
    ));

    let state = interp.initial_state();
    let mut context = interp.initial_context();
    let event = serde_json::json!({"type": "SUBMIT", "id": 9});

    let seen_guards = guard_calls.clone();
    let seen_actions = action_calls.clone();
    let guards = move |name: &str, ctx: &serde_json::Value, evt: &serde_json::Value| {
        seen_guards
            .lock()
            .unwrap()
            .push((name.to_string(), ctx.clone(), evt.clone()));
        true
    };
    let mut actions = move |name: &str, args: &[serde_json::Value], evt: &serde_json::Value| {
        seen_actions
            .lock()
            .unwrap()
            .push((name.to_string(), args.to_vec(), evt.clone()));
    };

    let next = interp.send(
        &state,
        &event,
        &mut context,
        &guards,
        &mut actions,
        &MockClock::new(0),
    );

    assert_eq!(next, Some(StateValue::Atomic("done".into())));
    assert_eq!(
        *guard_calls.lock().unwrap(),
        vec![(
            "canSubmit".to_string(),
            serde_json::json!({}),
            event.clone()
        )]
    );
    assert_eq!(
        *action_calls.lock().unwrap(),
        vec![(
            "log".to_string(),
            vec![serde_json::json!("extra")],
            event.clone()
        )]
    );
}

#[test]
fn missing_guard_allows_and_missing_action_is_noop() {
    assert!(GuardRegistry::new().evaluate("nope", &serde_json::json!({}), &serde_json::json!({})));

    let idle = {
        let mut t = trans("NEXT", "done");
        t.guard = Some(xstate_ir::Guard {
            name: "nope".into(),
        });
        t.actions = vec![Action {
            name: "nope".into(),
            args: vec![],
        }];
        atomic("idle", vec![t])
    };
    let interp = Interpreter::new(machine(
        "loose",
        "idle",
        vec![],
        vec![idle, atomic("done", vec![])],
    ));

    let state = interp.initial_state();
    let mut context = interp.initial_context();
    let next = interp.send_with_registry(
        &state,
        &serde_json::json!({"type": "NEXT"}),
        &mut context,
        &GuardRegistry::new(),
        &ActionRegistry::new(),
        &MockClock::new(0),
    );
    assert_eq!(
        next,
        Some(StateValue::Atomic("done".into())),
        "an unregistered guard must allow the transition"
    );

    let mut untouched = serde_json::json!({"touched": false});
    ActionRegistry::new().dispatch("nope", &[], &serde_json::json!({}), &mut untouched);
    assert_eq!(untouched, serde_json::json!({"touched": false}));
}
