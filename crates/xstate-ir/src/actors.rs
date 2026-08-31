use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActorDef {
    FromActorLogic { src: String },
    FromPromise { src: String },
    FromCallback { src: String },
    FromMachine { machine_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardDef {
    pub name: String,
    #[serde(default)]
    pub params: Vec<GuardParam>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardParam {
    Literal(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionDef {
    pub name: String,
    #[serde(default)]
    pub params: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SetupConfig {
    #[serde(default)]
    pub actors: HashMap<String, ActorDef>,
    #[serde(default)]
    pub guards: HashMap<String, GuardDef>,
    #[serde(default)]
    pub actions: HashMap<String, ActionDef>,
    #[serde(default)]
    pub delays: HashMap<String, u64>,
}

impl SetupConfig {
    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
            && self.guards.is_empty()
            && self.actions.is_empty()
            && self.delays.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_def_from_actor_logic_round_trip() {
        let def = ActorDef::FromActorLogic {
            src: "myLogic".into(),
        };
        let json = serde_json::to_string_pretty(&def).unwrap();
        let round_tripped: ActorDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, round_tripped);
    }

    #[test]
    fn actor_def_from_promise_round_trip() {
        let def = ActorDef::FromPromise {
            src: "loader".into(),
        };
        let json = serde_json::to_string_pretty(&def).unwrap();
        let round_tripped: ActorDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, round_tripped);
    }

    #[test]
    fn actor_def_from_callback_round_trip() {
        let def = ActorDef::FromCallback {
            src: "timer".into(),
        };
        let json = serde_json::to_string_pretty(&def).unwrap();
        let round_tripped: ActorDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, round_tripped);
    }

    #[test]
    fn actor_def_from_machine_round_trip() {
        let def = ActorDef::FromMachine {
            machine_id: "childMachine".into(),
        };
        let json = serde_json::to_string_pretty(&def).unwrap();
        let round_tripped: ActorDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, round_tripped);
    }

    #[test]
    fn guard_def_round_trip() {
        let def = GuardDef {
            name: "isValid".into(),
            params: vec![],
        };
        let json = serde_json::to_string_pretty(&def).unwrap();
        let round_tripped: GuardDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, round_tripped);
    }

    #[test]
    fn guard_def_with_params_round_trip() {
        let def = GuardDef {
            name: "isGreaterThan".into(),
            params: vec![GuardParam::Literal(serde_json::json!(0))],
        };
        let json = serde_json::to_string_pretty(&def).unwrap();
        let round_tripped: GuardDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, round_tripped);
    }

    #[test]
    fn action_def_round_trip() {
        let def = ActionDef {
            name: "assignCount".into(),
            params: vec![],
        };
        let json = serde_json::to_string_pretty(&def).unwrap();
        let round_tripped: ActionDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, round_tripped);
    }

    #[test]
    fn action_def_with_params_round_trip() {
        let def = ActionDef {
            name: "assign".into(),
            params: vec![serde_json::json!("count"), serde_json::json!(42)],
        };
        let json = serde_json::to_string_pretty(&def).unwrap();
        let round_tripped: ActionDef = serde_json::from_str(&json).unwrap();
        assert_eq!(def, round_tripped);
    }

    #[test]
    fn setup_config_round_trip() {
        let mut actors = HashMap::new();
        actors.insert(
            "loader".into(),
            ActorDef::FromPromise {
                src: "loader".into(),
            },
        );
        actors.insert(
            "timer".into(),
            ActorDef::FromCallback {
                src: "timer".into(),
            },
        );

        let mut guards = HashMap::new();
        guards.insert(
            "isValid".into(),
            GuardDef {
                name: "isValid".into(),
                params: vec![],
            },
        );

        let mut actions = HashMap::new();
        actions.insert(
            "assignCount".into(),
            ActionDef {
                name: "assignCount".into(),
                params: vec![serde_json::json!("count"), serde_json::json!(42)],
            },
        );

        let mut delays = HashMap::new();
        delays.insert("timeout".into(), 1000);

        let config = SetupConfig {
            actors,
            guards,
            actions,
            delays,
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let round_tripped: SetupConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, round_tripped);
    }

    #[test]
    fn setup_config_empty_round_trip() {
        let config = SetupConfig {
            actors: HashMap::new(),
            guards: HashMap::new(),
            actions: HashMap::new(),
            delays: HashMap::new(),
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let round_tripped: SetupConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, round_tripped);
    }
}
