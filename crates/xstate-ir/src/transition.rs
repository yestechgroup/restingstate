use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Transition {
    pub event: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub guard: Option<Guard>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Guard {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Action {
    pub name: String,
    #[serde(default)]
    pub args: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_round_trip() {
        let action = Action {
            name: "logEvent".into(),
            args: vec![],
        };
        let json = serde_json::to_string_pretty(&action).unwrap();
        let round_tripped: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(action, round_tripped);
    }

    #[test]
    fn action_with_args_round_trip() {
        let action = Action {
            name: "assign".into(),
            args: vec![serde_json::json!("count"), serde_json::json!(42)],
        };
        let json = serde_json::to_string_pretty(&action).unwrap();
        let round_tripped: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(action, round_tripped);
    }

    #[test]
    fn dynamic_assign_envelope_round_trip() {
        let action = Action {
            name: "assign".into(),
            args: vec![serde_json::json!({
                "$assign_ops": [
                    { "op": "add", "field": "count", "value": 1 },
                    { "op": "set", "field": "last", "from": "event.value" },
                    { "op": "set", "field": "nested.key", "value": "deep" }
                ]
            })],
        };
        let json = serde_json::to_string(&action).unwrap();
        let round_tripped: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(action, round_tripped);
        assert_eq!(
            round_tripped.args[0]["$assign_ops"][0]["op"],
            serde_json::json!("add")
        );
    }

    #[test]
    fn guard_round_trip() {
        let guard = Guard {
            name: "isAdmin".into(),
        };
        let json = serde_json::to_string_pretty(&guard).unwrap();
        let round_tripped: Guard = serde_json::from_str(&json).unwrap();
        assert_eq!(guard, round_tripped);
    }

    #[test]
    fn transition_basic_round_trip() {
        let transition = Transition {
            event: "toggle".into(),
            target: Some("active".into()),
            guard: None,
            actions: vec![],
            description: None,
            delay_ms: None,
            targets: vec![],
        };
        let json = serde_json::to_string_pretty(&transition).unwrap();
        let round_tripped: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(transition, round_tripped);
    }

    #[test]
    fn transition_with_guard_round_trip() {
        let transition = Transition {
            event: "submit".into(),
            target: Some("submitted".into()),
            guard: Some(Guard {
                name: "isValid".into(),
            }),
            actions: vec![],
            description: Some("Submit form if valid".into()),
            delay_ms: None,
            targets: vec![],
        };
        let json = serde_json::to_string_pretty(&transition).unwrap();
        let round_tripped: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(transition, round_tripped);
    }

    #[test]
    fn transition_with_multiple_actions_round_trip() {
        let transition = Transition {
            event: "fetch".into(),
            target: Some("loading".into()),
            guard: None,
            actions: vec![
                Action {
                    name: "startSpinner".into(),
                    args: vec![],
                },
                Action {
                    name: "assign".into(),
                    args: vec![serde_json::json!("status"), serde_json::json!("loading")],
                },
            ],
            description: None,
            delay_ms: None,
            targets: vec![],
        };
        let json = serde_json::to_string_pretty(&transition).unwrap();
        let round_tripped: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(transition, round_tripped);
    }

    #[test]
    fn transition_without_target_round_trip() {
        let transition = Transition {
            event: "log".into(),
            target: None,
            guard: None,
            actions: vec![Action {
                name: "logEvent".into(),
                args: vec![],
            }],
            description: None,
            delay_ms: None,
            targets: vec![],
        };
        let json = serde_json::to_string_pretty(&transition).unwrap();
        let round_tripped: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(transition, round_tripped);
    }

    #[test]
    fn transition_without_delay_round_trip() {
        let transition = Transition {
            event: "pause".into(),
            target: Some("paused".into()),
            guard: None,
            actions: vec![],
            description: None,
            delay_ms: None,
            targets: vec![],
        };
        let json = serde_json::to_string_pretty(&transition).unwrap();
        let round_tripped: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(transition, round_tripped);
    }

    #[test]
    fn transition_with_delay_round_trip() {
        let transition = Transition {
            event: "timeout".into(),
            target: Some("error".into()),
            guard: None,
            actions: vec![],
            description: Some("Delayed timeout".into()),
            delay_ms: Some(1000),
            targets: vec![],
        };
        let json = serde_json::to_string_pretty(&transition).unwrap();
        let round_tripped: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(transition, round_tripped);
    }

    #[test]
    fn transition_with_zero_delay_round_trip() {
        let transition = Transition {
            event: "immediate".into(),
            target: Some("next".into()),
            guard: None,
            actions: vec![],
            description: None,
            delay_ms: Some(0),
            targets: vec![],
        };
        let json = serde_json::to_string_pretty(&transition).unwrap();
        let round_tripped: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(transition, round_tripped);
    }
}
