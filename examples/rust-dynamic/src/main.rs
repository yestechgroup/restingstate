use serde_json::json;
use xstate_core::clock::MockClock;
use xstate_core::Interpreter;
use xstate_ir::{ContextField, ContextSchema, Machine, StateNode, StateType, Transition};

fn main() {
    let machine = Machine {
        id: "dynamicToggle".into(),
        initial: "inactive".into(),
        context: ContextSchema {
            fields: vec![ContextField {
                name: "count".into(),
                type_name: "number".into(),
                default_value: json!(0),
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
                    event: "TOGGLE".into(),
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
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
                invoke: None,
                on_done: vec![],
            },
        ],
    };

    let interpreter = Interpreter::new(machine);

    let state = interpreter.initial_state();
    let mut ctx = interpreter.initial_context();
    println!("Initial state: {:?}", state);
    println!("Initial context: {:?}", ctx);

    let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
    let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

    let new_state = interpreter
        .send(
            &state,
            &serde_json::json!({"type": "TOGGLE"}),
            &mut ctx,
            &guards,
            &mut actions,
            &MockClock::new(0),
        )
        .unwrap();
    println!("After TOGGLE: {:?}", new_state);

    let serialized = serde_json::to_string_pretty(&new_state).unwrap();
    println!("Serialized state: {}", serialized);
}
