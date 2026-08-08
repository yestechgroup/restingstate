use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub event: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub guard: Option<Guard>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guard {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        };
        let json = serde_json::to_string_pretty(&transition).unwrap();
        let round_tripped: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(transition, round_tripped);
    }
}
