use std::collections::HashMap;
use xstate_codegen::generate;
use xstate_ir::{
    ActionDef, ActorDef, ContextField, ContextSchema, Guard, GuardDef, Machine, SetupConfig,
    StateNode, StateType, Transition,
};

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

#[test]
fn golden_machine_with_setup_guards_and_actions() {
    let machine = Machine {
        id: "authForm".into(),
        initial: "idle".into(),
        context: ContextSchema {
            fields: vec![ContextField {
                name: "count".into(),
                type_name: "number".into(),
                default_value: serde_json::json!(0),
            }],
        },
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
                node_type: StateType::Final,
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
        setup: Some(SetupConfig {
            guards: {
                let mut m = HashMap::new();
                m.insert(
                    "isValid".into(),
                    GuardDef {
                        name: "isValid".into(),
                        params: vec![],
                    },
                );
                m
            },
            actions: {
                let mut m = HashMap::new();
                m.insert(
                    "assignCount".into(),
                    ActionDef {
                        name: "assignCount".into(),
                        params: vec![],
                    },
                );
                m
            },
            actors: HashMap::new(),
            delays: HashMap::new(),
        }),
    };

    let output = generate(&machine).unwrap().to_string();
    let n = normalize(&output);

    assert!(n.contains("pub mod guards"), "missing guards module");
    assert!(n.contains("pub mod actions"), "missing actions module");
    assert!(n.contains("fn is_valid"), "missing is_valid guard function");
    assert!(
        n.contains("fn assign_count"),
        "missing assign_count action function"
    );
    assert!(
        n.contains("Implement guard: isValid"),
        "missing isValid todo"
    );
    assert!(
        n.contains("Implement action: assignCount"),
        "missing assignCount todo"
    );
    assert!(n.contains("Idle"), "missing Idle state");
    assert!(n.contains("Submitted"), "missing Submitted state");
    assert!(n.contains("Submit"), "missing Submit event");
    assert!(
        n.contains("=> State :: Submitted"),
        "missing transition target"
    );
    assert!(n.contains("fn initial_state"), "missing initial_state fn");
    assert!(n.contains("State :: Idle"), "missing initial state");
    assert!(
        n.contains("type EventPayload = EventPayload"),
        "missing EventPayload associated type"
    );
}

#[test]
fn golden_machine_with_setup_actors() {
    let machine = Machine {
        id: "fetcher".into(),
        initial: "loading".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "loading".into(),
                transitions: vec![Transition {
                    event: "done.invoke.loader".into(),
                    target: Some("loaded".into()),
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
                invoke: Some(xstate_ir::InvokeConfig {
                    src: "loader".into(),
                    id: Some("loader".into()),
                    on_done: vec![Transition {
                        event: "done.invoke.loader".into(),
                        target: Some("loaded".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    }],
                    on_error: vec![],
                }),
                on_done: vec![],
                output: None,
                description: None,
            },
            StateNode {
                name: "loaded".into(),
                transitions: vec![],
                children: vec![],
                node_type: StateType::Final,
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
        setup: Some(SetupConfig {
            actors: {
                let mut m = HashMap::new();
                m.insert(
                    "loader".into(),
                    ActorDef::FromPromise {
                        src: "loader".into(),
                    },
                );
                m
            },
            guards: HashMap::new(),
            actions: HashMap::new(),
            delays: HashMap::new(),
        }),
    };

    let output = generate(&machine).unwrap().to_string();
    let n = normalize(&output);

    assert!(n.contains("pub mod actors"), "missing actors module");
    assert!(n.contains("fn load_loader"), "missing load_loader function");
    assert!(n.contains("ActorLogic"), "missing ActorLogic trait bound");
    assert!(n.contains("Implement actor: loader"), "missing loader todo");
    assert!(
        !n.contains("pub mod guards"),
        "should not have guards module when no guards"
    );
    assert!(
        !n.contains("pub mod actions"),
        "should not have actions module when no actions"
    );
    assert!(n.contains("Loading"), "missing Loading state");
    assert!(n.contains("Loaded"), "missing Loaded state");
    assert!(
        n.contains("DoneInvokeLoader"),
        "missing DoneInvokeLoader event variant"
    );
    assert!(
        n.contains("=> State :: Loaded"),
        "missing invoke done transition target"
    );
}

#[test]
fn golden_machine_with_full_setup() {
    let machine = Machine {
        id: "fullSetup".into(),
        initial: "idle".into(),
        context: ContextSchema {
            fields: vec![ContextField {
                name: "count".into(),
                type_name: "number".into(),
                default_value: serde_json::json!(0),
            }],
        },
        states: vec![
            StateNode {
                name: "idle".into(),
                transitions: vec![Transition {
                    event: "START".into(),
                    target: Some("running".into()),
                    guard: Some(Guard {
                        name: "canStart".into(),
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
                name: "running".into(),
                transitions: vec![Transition {
                    event: "done.invoke.worker".into(),
                    target: Some("done".into()),
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
                invoke: Some(xstate_ir::InvokeConfig {
                    src: "worker".into(),
                    id: Some("worker".into()),
                    on_done: vec![Transition {
                        event: "done.invoke.worker".into(),
                        target: Some("done".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    }],
                    on_error: vec![],
                }),
                on_done: vec![],
                output: None,
                description: None,
            },
            StateNode {
                name: "done".into(),
                transitions: vec![],
                children: vec![],
                node_type: StateType::Final,
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
        setup: Some(SetupConfig {
            actors: {
                let mut m = HashMap::new();
                m.insert(
                    "worker".into(),
                    ActorDef::FromPromise {
                        src: "worker".into(),
                    },
                );
                m
            },
            guards: {
                let mut m = HashMap::new();
                m.insert(
                    "canStart".into(),
                    GuardDef {
                        name: "canStart".into(),
                        params: vec![],
                    },
                );
                m
            },
            actions: {
                let mut m = HashMap::new();
                m.insert(
                    "assignCount".into(),
                    ActionDef {
                        name: "assignCount".into(),
                        params: vec![serde_json::json!("count"), serde_json::json!(1)],
                    },
                );
                m
            },
            delays: HashMap::new(),
        }),
    };

    let output = generate(&machine).unwrap().to_string();
    let n = normalize(&output);

    assert!(n.contains("pub mod guards"), "missing guards module");
    assert!(n.contains("pub mod actions"), "missing actions module");
    assert!(n.contains("pub mod actors"), "missing actors module");
    assert!(n.contains("fn can_start"), "missing can_start guard");
    assert!(n.contains("fn assign_count"), "missing assign_count action");
    assert!(n.contains("fn load_worker"), "missing load_worker actor");
}

#[test]
fn compile_test_actor_aware_machine() {
    let machine = Machine {
        id: "test".into(),
        initial: "idle".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "idle".into(),
                transitions: vec![Transition {
                    event: "GO".into(),
                    target: Some("active".into()),
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
        setup: Some(SetupConfig {
            guards: {
                let mut m = HashMap::new();
                m.insert(
                    "isValid".into(),
                    GuardDef {
                        name: "isValid".into(),
                        params: vec![],
                    },
                );
                m
            },
            actions: {
                let mut m = HashMap::new();
                m.insert(
                    "increment".into(),
                    ActionDef {
                        name: "increment".into(),
                        params: vec![],
                    },
                );
                m
            },
            actors: HashMap::new(),
            delays: HashMap::new(),
        }),
    };

    let tokens = generate(&machine).unwrap();
    let code = tokens.to_string();
    assert!(code.contains("pub mod guards"));
    assert!(code.contains("pub mod actions"));
    assert!(code.contains("fn is_valid"));
    assert!(code.contains("fn increment"));
    let file: syn::File = syn::parse_str(&code).expect("Generated code should be valid Rust");
    assert!(!file.items.is_empty());
}
