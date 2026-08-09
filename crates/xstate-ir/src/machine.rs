use serde::{Deserialize, Serialize};

use crate::state::StateNode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    pub id: String,
    pub initial: String,
    pub context: ContextSchema,
    pub states: Vec<StateNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSchema {
    pub fields: Vec<ContextField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextField {
    pub name: String,
    pub type_name: String,
    pub default_value: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateType;
    use crate::transition::Transition;

    #[test]
    fn context_field_round_trip() {
        let field = ContextField {
            name: "count".into(),
            type_name: "number".into(),
            default_value: serde_json::json!(0),
        };
        let json = serde_json::to_string_pretty(&field).unwrap();
        let round_tripped: ContextField = serde_json::from_str(&json).unwrap();
        assert_eq!(field, round_tripped);
    }

    #[test]
    fn context_schema_round_trip() {
        let schema = ContextSchema {
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
        };
        let json = serde_json::to_string_pretty(&schema).unwrap();
        let round_tripped: ContextSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(schema, round_tripped);
    }

    #[test]
    fn simple_atomic_toggle_machine_round_trip() {
        let machine = Machine {
            id: "toggle".into(),
            initial: "inactive".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                StateNode {
                    name: "inactive".into(),
                    description: Some("Inactive state".into()),
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
                    description: Some("Active state".into()),
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
        let json = serde_json::to_string_pretty(&machine).unwrap();
        let round_tripped: Machine = serde_json::from_str(&json).unwrap();
        assert_eq!(machine, round_tripped);
    }

    #[test]
    fn compound_state_machine_round_trip() {
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
                        targets: vec![],
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
                                targets: vec![],
                            }],
                            children: vec![],
                            node_type: StateType::Atomic,
                            tags: vec![],
                            invoke: None,
                            on_done: vec![],
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
                            invoke: None,
                            on_done: vec![],
                        },
                    ],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                },
                StateNode {
                    name: "green".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "timer".into(),
                        target: Some("yellow".into()),
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
                    name: "yellow".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "timer".into(),
                        target: Some("red".into()),
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
        let json = serde_json::to_string_pretty(&machine).unwrap();
        let round_tripped: Machine = serde_json::from_str(&json).unwrap();
        assert_eq!(machine, round_tripped);
    }

    #[test]
    fn machine_with_parallel_states_round_trip() {
        let machine = Machine {
            id: "dashboard".into(),
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
                        invoke: None,
                        on_done: vec![],
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
                        invoke: None,
                        on_done: vec![],
                    },
                ],
                node_type: StateType::Parallel,
                tags: vec![],
                invoke: None,
                on_done: vec![],
            }],
        };
        let json = serde_json::to_string_pretty(&machine).unwrap();
        let round_tripped: Machine = serde_json::from_str(&json).unwrap();
        assert_eq!(machine, round_tripped);
    }
}
