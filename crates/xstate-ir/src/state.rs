use serde::{Deserialize, Serialize};

use crate::transition::Transition;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InvokeConfig {
    pub src: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub on_done: Vec<Transition>,
    #[serde(default)]
    pub on_error: Vec<Transition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StateNode {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub initial: Option<String>,
    #[serde(default)]
    pub entry: Vec<String>,
    #[serde(default)]
    pub exit: Vec<String>,
    #[serde(default)]
    pub transitions: Vec<Transition>,
    #[serde(default)]
    pub children: Vec<StateNode>,
    #[serde(rename = "type", default)]
    pub node_type: StateType,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub invoke: Option<InvokeConfig>,
    #[serde(default)]
    pub on_done: Vec<Transition>,
    #[serde(default)]
    pub output: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum StateType {
    #[default]
    Atomic,
    Compound,
    Parallel,
    Final,
    History,
    DeepHistory,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_type_variants_round_trip() {
        let variants = vec![
            (StateType::Atomic, "\"atomic\""),
            (StateType::Compound, "\"compound\""),
            (StateType::Parallel, "\"parallel\""),
            (StateType::Final, "\"final\""),
            (StateType::History, "\"history\""),
            (StateType::DeepHistory, "\"deephistory\""),
        ];
        for (variant, expected_json) in variants {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_json);
            let round_tripped: StateType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, round_tripped);
        }
    }

    #[test]
    fn state_node_atomic_round_trip() {
        let state = StateNode {
            name: "idle".into(),
            description: Some("Idle state".into()),
            initial: None,
            entry: vec![],
            exit: vec![],
            transitions: vec![],
            children: vec![],
            node_type: StateType::Atomic,
            tags: vec![],
            invoke: None,
            on_done: vec![],
            output: None,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let round_tripped: StateNode = serde_json::from_str(&json).unwrap();
        assert_eq!(state, round_tripped);
    }

    #[test]
    fn state_node_with_entry_exit_actions_round_trip() {
        let state = StateNode {
            name: "loading".into(),
            description: None,
            initial: None,
            entry: vec!["startSpinner".into(), "trackPageView".into()],
            exit: vec!["stopSpinner".into()],
            transitions: vec![],
            children: vec![],
            node_type: StateType::Atomic,
            tags: vec![],
            invoke: None,
            on_done: vec![],
            output: None,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let round_tripped: StateNode = serde_json::from_str(&json).unwrap();
        assert_eq!(state, round_tripped);
    }

    #[test]
    fn state_node_compound_with_children_round_trip() {
        let red_state = StateNode {
            name: "red".into(),
            description: Some("Red light".into()),
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
                    output: None,
                },
                StateNode {
                    name: "stop".into(),
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
                    output: None,
                },
            ],
            node_type: StateType::Compound,
            tags: vec!["critical".into()],
            invoke: None,
            on_done: vec![],
            output: None,
        };
        let json = serde_json::to_string_pretty(&red_state).unwrap();
        let round_tripped: StateNode = serde_json::from_str(&json).unwrap();
        assert_eq!(red_state, round_tripped);
    }

    #[test]
    fn state_node_parallel_round_trip() {
        let state = StateNode {
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
                    output: None,
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
                    output: None,
                },
            ],
            node_type: StateType::Parallel,
            tags: vec![],
            invoke: None,
            on_done: vec![],
            output: None,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let round_tripped: StateNode = serde_json::from_str(&json).unwrap();
        assert_eq!(state, round_tripped);
    }

    #[test]
    fn state_node_final_round_trip() {
        let state = StateNode {
            name: "done".into(),
            description: Some("Final terminal state".into()),
            initial: None,
            entry: vec![],
            exit: vec![],
            transitions: vec![],
            children: vec![],
            node_type: StateType::Final,
            tags: vec![],
            invoke: None,
            on_done: vec![],
            output: None,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let round_tripped: StateNode = serde_json::from_str(&json).unwrap();
        assert_eq!(state, round_tripped);
    }

    #[test]
    fn state_node_history_round_trip() {
        let state = StateNode {
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
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let round_tripped: StateNode = serde_json::from_str(&json).unwrap();
        assert_eq!(state, round_tripped);
    }

    #[test]
    fn state_node_deep_history_round_trip() {
        let state = StateNode {
            name: "deepHist".into(),
            description: None,
            initial: None,
            entry: vec![],
            exit: vec![],
            transitions: vec![],
            children: vec![],
            node_type: StateType::DeepHistory,
            tags: vec![],
            invoke: None,
            on_done: vec![],
            output: None,
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let round_tripped: StateNode = serde_json::from_str(&json).unwrap();
        assert_eq!(state, round_tripped);
    }

    #[test]
    fn invoke_config_round_trip() {
        let config = InvokeConfig {
            src: "fetchData".into(),
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
            on_error: vec![Transition {
                event: "error.platform.loader".into(),
                target: Some("failed".into()),
                guard: None,
                actions: vec![],
                description: None,
                delay_ms: None,
                targets: vec![],
            }],
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let round_tripped: InvokeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, round_tripped);
    }

    #[test]
    fn invoke_config_without_id_round_trip() {
        let config = InvokeConfig {
            src: "fetchData".into(),
            id: None,
            on_done: vec![],
            on_error: vec![],
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let round_tripped: InvokeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, round_tripped);
    }

    #[test]
    fn state_node_with_invoke_round_trip() {
        let state = StateNode {
            name: "loading".into(),
            description: None,
            initial: None,
            entry: vec![],
            exit: vec![],
            transitions: vec![],
            children: vec![],
            node_type: StateType::Atomic,
            tags: vec![],
            invoke: Some(InvokeConfig {
                src: "fetchData".into(),
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
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let round_tripped: StateNode = serde_json::from_str(&json).unwrap();
        assert_eq!(state, round_tripped);
    }
}
