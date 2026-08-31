use xstate_codegen::generate;
use xstate_ir::{ContextField, ContextSchema, Guard, Machine, StateNode, StateType, Transition};

fn normalize(src: &str) -> String {
    src.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn golden_simple_toggle() {
    let machine = Machine {
        id: "toggle".into(),
        initial: "inactive".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "inactive".into(),
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
                entry: vec![],
                exit: vec![],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
            StateNode {
                name: "active".into(),
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
                entry: vec![],
                exit: vec![],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
        ],

        setup: None,
    };

    let output = generate(&machine).unwrap().to_string();
    let n = normalize(&output);

    assert!(n.contains("pub enum State"), "missing State enum");
    assert!(n.contains("Inactive"), "missing Inactive variant");
    assert!(n.contains("Active"), "missing Active variant");
    assert!(n.contains("pub enum Event"), "missing Event enum");
    assert!(n.contains("Toggle"), "missing Toggle variant");
    assert!(
        n.contains("Inactive , Event :: Toggle"),
        "missing Inactive/TOGGLE match arm"
    );
    assert!(
        n.contains("Active , Event :: Toggle"),
        "missing Active/TOGGLE match arm"
    );
    assert!(
        n.contains("=> State :: Active"),
        "missing transition to Active"
    );
    assert!(
        n.contains("=> State :: Inactive"),
        "missing transition to Inactive"
    );
    assert!(n.contains("fn initial_state"), "missing initial_state fn");
    assert!(
        n.contains("State :: Inactive"),
        "missing initial state value"
    );
    assert!(n.contains("Serialize"), "missing Serialize derive");
    assert!(n.contains("Deserialize"), "missing Deserialize derive");
    assert!(
        n.contains("impl :: xstate_core :: Machine"),
        "missing Machine impl"
    );
    assert!(
        n.contains("type EventPayload = EventPayload"),
        "missing EventPayload associated type"
    );
    assert!(
        n.contains("_payload : & Self :: EventPayload"),
        "missing _payload parameter in transition"
    );
    assert!(
        n.contains("pub type EventPayload = :: serde_json :: Value"),
        "missing EventPayload type alias"
    );
}

#[test]
fn golden_traffic_light() {
    let machine = Machine {
        id: "trafficLight".into(),
        initial: "green".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "green".into(),
                transitions: vec![Transition {
                    event: "TIMER".into(),
                    target: Some("yellow".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                entry: vec![],
                exit: vec![],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
            StateNode {
                name: "yellow".into(),
                transitions: vec![Transition {
                    event: "TIMER".into(),
                    target: Some("red".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                entry: vec![],
                exit: vec![],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
            StateNode {
                name: "red".into(),
                transitions: vec![Transition {
                    event: "TIMER".into(),
                    target: Some("green".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                entry: vec![],
                exit: vec![],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
        ],

        setup: None,
    };

    let output = generate(&machine).unwrap().to_string();
    let n = normalize(&output);

    assert!(n.contains("pub enum State"));
    assert!(n.contains("Green"));
    assert!(n.contains("Yellow"));
    assert!(n.contains("Red"));
    assert!(n.contains("pub enum Event"));
    assert!(n.contains("Timer"));
    assert!(
        n.contains("Green , Event :: Timer"),
        "missing green→yellow transition"
    );
    assert!(n.contains("=> State :: Yellow"), "missing green target");
    assert!(
        n.contains("Yellow , Event :: Timer"),
        "missing yellow→red transition"
    );
    assert!(n.contains("=> State :: Red"), "missing yellow target");
    assert!(
        n.contains("Red , Event :: Timer"),
        "missing red→green transition"
    );
    assert!(n.contains("=> State :: Green"), "missing red target");
    assert!(n.contains("fn initial_state"));
    assert!(n.contains("State :: Green"), "missing initial state Green");
    assert!(
        n.contains("type EventPayload = EventPayload"),
        "missing EventPayload associated type"
    );
    assert!(
        n.contains("_payload : & Self :: EventPayload"),
        "missing _payload parameter in transition"
    );
    assert!(
        n.contains("pub type EventPayload = :: serde_json :: Value"),
        "missing EventPayload type alias"
    );
}

#[test]
fn golden_with_context() {
    let machine = Machine {
        id: "counter".into(),
        initial: "idle".into(),
        context: ContextSchema {
            fields: vec![
                ContextField {
                    name: "count".into(),
                    type_name: "number".into(),
                    default_value: serde_json::json!(0),
                },
                ContextField {
                    name: "message".into(),
                    type_name: "string".into(),
                    default_value: serde_json::json!("hello"),
                },
            ],
        },
        states: vec![StateNode {
            name: "idle".into(),
            transitions: vec![],
            children: vec![],
            node_type: StateType::Atomic,
            entry: vec![],
            exit: vec![],
            initial: None,
            tags: vec![],
            invoke: None,
            on_done: vec![],
            output: None,
            description: None,
        }],

        setup: None,
    };

    let output = generate(&machine).unwrap().to_string();
    let n = normalize(&output);

    assert!(n.contains("pub struct Context"), "missing Context struct");
    assert!(n.contains("pub count : i64"), "missing count field");
    assert!(n.contains("pub message : String"), "missing message field");
    assert!(
        n.contains("Serialize"),
        "missing Serialize derive on Context"
    );
    assert!(
        n.contains("Deserialize"),
        "missing Deserialize derive on Context"
    );
    assert!(n.contains("Clone"), "missing Clone derive on Context");
    assert!(
        n.contains("type EventPayload = EventPayload"),
        "missing EventPayload associated type"
    );
    assert!(
        n.contains("_payload : & Self :: EventPayload"),
        "missing _payload parameter in transition"
    );
}

#[test]
fn golden_with_guards() {
    let machine = Machine {
        id: "form".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "idle".into(),
                transitions: vec![Transition {
                    event: "SUBMIT".into(),
                    target: Some("submitted".into()),
                    guard: Some(Guard {
                        name: "isValid".into(),
                    }),
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                entry: vec![],
                exit: vec![],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
            StateNode {
                name: "submitted".into(),
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                entry: vec![],
                exit: vec![],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
        ],

        setup: None,
    };

    let output = generate(&machine).unwrap().to_string();
    let n = normalize(&output);

    assert!(n.contains("pub enum Event"), "missing Event enum");
    assert!(n.contains("Submit"), "missing Submit event variant");
    assert!(n.contains("Idle"), "missing Idle state variant");
    assert!(n.contains("Submitted"), "missing Submitted state variant");
    assert!(
        n.contains("Idle , Event :: Submit"),
        "missing Idle/SUBMIT match arm"
    );
    assert!(
        n.contains("=> State :: Submitted"),
        "missing transition target"
    );
    assert!(
        n.contains("type EventPayload = EventPayload"),
        "missing EventPayload associated type"
    );
    assert!(
        n.contains("_payload : & Self :: EventPayload"),
        "missing _payload parameter in transition"
    );
}

#[test]
fn golden_with_entry_exit_actions() {
    let machine = Machine {
        id: "loading".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "idle".into(),
                transitions: vec![Transition {
                    event: "START".into(),
                    target: Some("loading".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                entry: vec![],
                exit: vec!["logExit".into()],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
            StateNode {
                name: "loading".into(),
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                entry: vec!["startSpinner".into(), "trackPageView".into()],
                exit: vec!["stopSpinner".into()],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
        ],

        setup: None,
    };

    let output = generate(&machine).unwrap().to_string();
    let n = normalize(&output);

    assert!(n.contains("pub enum State"), "missing State enum");
    assert!(n.contains("Idle"), "missing Idle state");
    assert!(n.contains("Loading"), "missing Loading state");
    assert!(n.contains("pub enum Event"), "missing Event enum");
    assert!(n.contains("Start"), "missing Start event");
    assert!(
        n.contains("Idle , Event :: Start"),
        "missing Idle/START match arm"
    );
    assert!(
        n.contains("=> State :: Loading"),
        "missing transition target"
    );
    assert!(n.contains("fn initial_state"), "missing initial_state fn");
    assert!(n.contains("State :: Idle"), "missing initial state");
    assert!(
        n.contains("type EventPayload = EventPayload"),
        "missing EventPayload associated type"
    );
    assert!(
        n.contains("_payload : & Self :: EventPayload"),
        "missing _payload parameter in transition"
    );
}

#[test]
fn golden_excludes_delayed_transitions_from_event_enum() {
    let machine = Machine {
        id: "afterTest".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "idle".into(),
                transitions: vec![
                    Transition {
                        event: "after".into(),
                        target: Some("active".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: Some(1000),
                        targets: vec![],
                    },
                    Transition {
                        event: "MANUAL".into(),
                        target: Some("active".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    },
                ],
                children: vec![],
                node_type: StateType::Atomic,
                entry: vec![],
                exit: vec![],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
            StateNode {
                name: "active".into(),
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                entry: vec![],
                exit: vec![],
                initial: None,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
                description: None,
            },
        ],

        setup: None,
    };

    let output = generate(&machine).unwrap().to_string();
    let n = normalize(&output);

    assert!(n.contains("pub enum Event"), "missing Event enum");
    assert!(n.contains("Manual"), "manual event should be in the enum");
    assert!(
        !n.contains("Event :: After") && !n.contains("Event::After"),
        "delayed transition event should not appear in event enum"
    );
}
