use xstate_core::clock::MockClock;
use xstate_core::Interpreter;
use xstate_core::StateValue;
use xstate_ir::{
    Action, ContextField, ContextSchema, Guard, Machine, StateNode, StateType, Transition,
};

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
                    event: "toggle".into(),
                    target: Some("active".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
            StateNode {
                name: "active".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "toggle".into(),
                    target: Some("inactive".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
        ],
    }
}

#[test]
fn toggle_machine_basic_transitions() {
    let machine = toggle_machine();
    let interpreter = Interpreter::new(machine);
    let initial = interpreter.initial_state();
    assert_eq!(initial, StateValue::Atomic("inactive".into()));

    let mut ctx = interpreter.initial_context();
    let no_guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
    let mut no_actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

    let after_toggle = interpreter.send(
        &initial,
        &serde_json::json!({"type": "toggle"}),
        &mut ctx,
        &no_guards,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(after_toggle, Some(StateValue::Atomic("active".into())));

    let after_toggle_again = interpreter.send(
        &StateValue::Atomic("active".into()),
        &serde_json::json!({"type": "toggle"}),
        &mut ctx,
        &no_guards,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(
        after_toggle_again,
        Some(StateValue::Atomic("inactive".into()))
    );

    let unknown = interpreter.send(
        &StateValue::Atomic("inactive".into()),
        &serde_json::json!({"type": "unknown"}),
        &mut ctx,
        &no_guards,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(unknown, None);
}

#[test]
fn toggle_machine_with_count_context() {
    let machine = Machine {
        id: "toggle".into(),
        initial: "inactive".into(),
        context: ContextSchema {
            fields: vec![ContextField {
                name: "count".into(),
                type_name: "number".into(),
                default_value: serde_json::json!(0),
            }],
        },
        states: vec![
            StateNode {
                name: "inactive".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "toggle".into(),
                    target: Some("active".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
            StateNode {
                name: "active".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "toggle".into(),
                    target: Some("inactive".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
        ],
    };
    let interpreter = Interpreter::new(machine);
    let ctx = interpreter.initial_context();
    assert_eq!(ctx, serde_json::json!({"count": 0}));
}

#[test]
fn traffic_light_compound_state() {
    let machine = Machine {
        id: "trafficLight".into(),
        initial: "red".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "red".into(),
                description: None,
                initial: Some("walk".into()),
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "timer".into(),
                    target: Some("green".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                }],
                children: vec![
                    StateNode {
                        name: "walk".into(),
                        description: None,
                        initial: None,
                        entry: vec![],
                        exit: vec![],
                        transitions: vec![Transition {
                            event: "reset".into(),
                            target: Some("stop".into()),
                            guard: None,
                            actions: vec![],
                            description: None,
                            delay_ms: None,
                        }],
                        children: vec![],
                        node_type: StateType::Atomic,
                        tags: vec![],
                    },
                    StateNode {
                        name: "stop".into(),
                        description: None,
                        initial: None,
                        entry: vec![],
                        exit: vec![],
                        transitions: vec![],
                        children: vec![],
                        node_type: StateType::Final,
                        tags: vec![],
                    },
                ],
                node_type: StateType::Compound,
                tags: vec![],
            },
            StateNode {
                name: "green".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
        ],
    };
    let interpreter = Interpreter::new(machine);
    let initial = interpreter.initial_state();
    assert_eq!(
        initial,
        StateValue::Compound {
            state: "red".into(),
            child: Box::new(StateValue::Atomic("walk".into())),
        }
    );

    let mut ctx = interpreter.initial_context();
    let no_guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
    let mut no_actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

    let after_timer = interpreter.send(
        &initial,
        &serde_json::json!({"type": "timer"}),
        &mut ctx,
        &no_guards,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(after_timer, Some(StateValue::Atomic("green".into())));

    let after_reset = interpreter.send(
        &initial,
        &serde_json::json!({"type": "reset"}),
        &mut ctx,
        &no_guards,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(
        after_reset,
        Some(StateValue::Compound {
            state: "red".into(),
            child: Box::new(StateValue::Atomic("stop".into())),
        })
    );

    let red_stop = StateValue::Compound {
        state: "red".into(),
        child: Box::new(StateValue::Atomic("stop".into())),
    };
    let from_final = interpreter.send(
        &red_stop,
        &serde_json::json!({"type": "reset"}),
        &mut ctx,
        &no_guards,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(from_final, None);

    let from_final_timer = interpreter.send(
        &red_stop,
        &serde_json::json!({"type": "timer"}),
        &mut ctx,
        &no_guards,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(from_final_timer, Some(StateValue::Atomic("green".into())));
}

#[test]
fn transition_with_guard() {
    let machine = Machine {
        id: "guardTest".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "idle".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![
                    Transition {
                        event: "SUBMIT".into(),
                        target: Some("submitted".into()),
                        guard: Some(Guard {
                            name: "isValid".into(),
                        }),
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                    },
                    Transition {
                        event: "SUBMIT".into(),
                        target: Some("error".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                    },
                ],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
            StateNode {
                name: "submitted".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
            StateNode {
                name: "error".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
        ],
    };
    let interpreter = Interpreter::new(machine);
    let initial = interpreter.initial_state();
    let mut ctx = interpreter.initial_context();
    let mut no_actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

    let guard_true = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
    let result_true = interpreter.send(
        &initial,
        &serde_json::json!({"type": "SUBMIT"}),
        &mut ctx,
        &guard_true,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(result_true, Some(StateValue::Atomic("submitted".into())));

    let guard_false = |_: &str, _: &serde_json::Value, _: &serde_json::Value| false;
    let result_false = interpreter.send(
        &initial,
        &serde_json::json!({"type": "SUBMIT"}),
        &mut ctx,
        &guard_false,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(result_false, Some(StateValue::Atomic("error".into())));
}

#[test]
fn entry_exit_actions() {
    let machine = Machine {
        id: "actionsTest".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "idle".into(),
                description: None,
                initial: None,
                entry: vec!["onIdleEnter".into()],
                exit: vec!["onIdleExit".into()],
                transitions: vec![Transition {
                    event: "toggle".into(),
                    target: Some("active".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
            StateNode {
                name: "active".into(),
                description: None,
                initial: None,
                entry: vec!["onActiveEnter".into()],
                exit: vec!["onActiveExit".into()],
                transitions: vec![Transition {
                    event: "toggle".into(),
                    target: Some("idle".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
        ],
    };
    let interpreter = Interpreter::new(machine);
    let initial = interpreter.initial_state();
    let mut ctx = interpreter.initial_context();
    let no_guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

    let mut actions_log: Vec<String> = Vec::new();
    let mut log_actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
        actions_log.push(name.to_string());
    };

    let _result = interpreter.send(
        &initial,
        &serde_json::json!({"type": "toggle"}),
        &mut ctx,
        &no_guards,
        &mut log_actions,
        &MockClock::new(0),
    );

    assert_eq!(actions_log.len(), 2);
    assert_eq!(actions_log[0], "onIdleExit");
    assert_eq!(actions_log[1], "onActiveEnter");
}

#[test]
fn transition_actions_order() {
    let machine = Machine {
        id: "transActions".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "idle".into(),
                description: None,
                initial: None,
                entry: vec!["onIdleEnter".into()],
                exit: vec!["onIdleExit".into()],
                transitions: vec![Transition {
                    event: "toggle".into(),
                    target: Some("active".into()),
                    guard: None,
                    actions: vec![Action {
                        name: "transitionAction".into(),
                        args: vec![],
                    }],
                    description: None,
                    delay_ms: None,
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
            StateNode {
                name: "active".into(),
                description: None,
                initial: None,
                entry: vec!["onActiveEnter".into()],
                exit: vec!["onActiveExit".into()],
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
        ],
    };
    let interpreter = Interpreter::new(machine);
    let initial = interpreter.initial_state();
    let mut ctx = interpreter.initial_context();
    let no_guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

    let mut actions_log: Vec<String> = Vec::new();
    let mut log_actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
        actions_log.push(name.to_string());
    };

    let _result = interpreter.send(
        &initial,
        &serde_json::json!({"type": "toggle"}),
        &mut ctx,
        &no_guards,
        &mut log_actions,
        &MockClock::new(0),
    );

    assert_eq!(actions_log.len(), 3);
    assert_eq!(actions_log[0], "onIdleExit");
    assert_eq!(actions_log[1], "transitionAction");
    assert_eq!(actions_log[2], "onActiveEnter");
}

#[test]
fn parallel_states_initial_value() {
    let machine = Machine {
        id: "parallel".into(),
        initial: "loading".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![StateNode {
            name: "loading".into(),
            description: None,
            initial: None,
            entry: vec![],
            exit: vec![],
            transitions: vec![],
            children: vec![
                StateNode {
                    name: "fetching".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
                StateNode {
                    name: "rendering".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
            ],
            node_type: StateType::Parallel,
            tags: vec![],
        }],
    };
    let interpreter = Interpreter::new(machine);
    let initial = interpreter.initial_state();

    let expected = StateValue::Parallel {
        regions: vec![
            ("fetching".into(), StateValue::Atomic("fetching".into())),
            ("rendering".into(), StateValue::Atomic("rendering".into())),
        ],
    };
    assert_eq!(initial, expected);
}

#[test]
fn event_bubbling() {
    let machine = Machine {
        id: "bubble".into(),
        initial: "parent".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "parent".into(),
                description: None,
                initial: Some("child1".into()),
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "ping".into(),
                    target: Some("other".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                }],
                children: vec![StateNode {
                    name: "child1".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                }],
                node_type: StateType::Compound,
                tags: vec![],
            },
            StateNode {
                name: "other".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            },
        ],
    };
    let interpreter = Interpreter::new(machine);
    let initial = interpreter.initial_state();
    assert_eq!(
        initial,
        StateValue::Compound {
            state: "parent".into(),
            child: Box::new(StateValue::Atomic("child1".into())),
        }
    );

    let mut ctx = interpreter.initial_context();
    let no_guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
    let mut no_actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

    let result = interpreter.send(
        &initial,
        &serde_json::json!({"type": "ping"}),
        &mut ctx,
        &no_guards,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(result, Some(StateValue::Atomic("other".into())));
}

#[test]
fn transition_with_no_target() {
    let machine = Machine {
        id: "noTarget".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![StateNode {
            name: "idle".into(),
            description: None,
            initial: None,
            entry: vec![],
            exit: vec![],
            transitions: vec![Transition {
                event: "log".into(),
                target: None,
                guard: None,
                actions: vec![Action {
                    name: "logEvent".into(),
                    args: vec![serde_json::json!("event")],
                }],
                description: None,
                delay_ms: None,
            }],
            children: vec![],
            node_type: StateType::Atomic,
            tags: vec![],
        }],
    };
    let interpreter = Interpreter::new(machine);
    let initial = interpreter.initial_state();
    let mut ctx = interpreter.initial_context();
    let no_guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

    let mut actions_called = false;
    let mut check_action = |name: &str, args: &[serde_json::Value], _: &serde_json::Value| {
        assert_eq!(name, "logEvent");
        assert_eq!(args, &[serde_json::json!("event")]);
        actions_called = true;
    };

    let result = interpreter.send(
        &initial,
        &serde_json::json!({"type": "log"}),
        &mut ctx,
        &no_guards,
        &mut check_action,
        &MockClock::new(0),
    );
    assert!(actions_called);
    assert_eq!(result, Some(StateValue::Atomic("idle".into())));
}

#[test]
fn final_state_no_transitions() {
    let machine = Machine {
        id: "finalTest".into(),
        initial: "done".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![StateNode {
            name: "done".into(),
            description: None,
            initial: None,
            entry: vec![],
            exit: vec![],
            transitions: vec![Transition {
                event: "RESTART".into(),
                target: Some("idle".into()),
                guard: None,
                actions: vec![],
                description: None,
                delay_ms: None,
            }],
            children: vec![],
            node_type: StateType::Final,
            tags: vec![],
        }],
    };
    let interpreter = Interpreter::new(machine);
    let initial = interpreter.initial_state();
    let mut ctx = interpreter.initial_context();
    let no_guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
    let mut no_actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

    let result = interpreter.send(
        &initial,
        &serde_json::json!({"type": "RESTART"}),
        &mut ctx,
        &no_guards,
        &mut no_actions,
        &MockClock::new(0),
    );
    assert_eq!(result, None);
}
