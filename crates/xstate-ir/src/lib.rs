pub mod actors;
pub mod machine;
#[cfg(feature = "schema")]
pub mod schema;
pub mod state;
pub mod transition;

pub use actors::{ActionDef, ActorDef, GuardDef, SetupConfig};
pub use machine::{ContextField, ContextSchema, Machine};
#[cfg(feature = "schema")]
pub use schema::{machine_ir_schema, machine_ir_schema_json};
pub use state::{InvokeConfig, StateNode, StateType};
pub use transition::{Action, Guard, Transition};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_machine_end_to_end_round_trip() {
        let machine = Machine {
            id: "paymentMachine".into(),
            initial: "idle".into(),
            context: ContextSchema {
                fields: vec![
                    ContextField {
                        name: "amount".into(),
                        type_name: "number".into(),
                        default_value: serde_json::json!(0),
                    },
                    ContextField {
                        name: "currency".into(),
                        type_name: "string".into(),
                        default_value: serde_json::json!("USD"),
                    },
                ],
            },
            states: vec![
                StateNode {
                    name: "idle".into(),
                    description: Some("Waiting for payment request".into()),
                    initial: None,
                    entry: vec!["resetForm".into()],
                    exit: vec!["logExit".into()],
                    transitions: vec![Transition {
                        event: "PAY".into(),
                        target: Some("processing".into()),
                        guard: Some(Guard {
                            name: "isAmountValid".into(),
                        }),
                        actions: vec![
                            Action {
                                name: "assign".into(),
                                args: vec![serde_json::json!("amount"), serde_json::json!(100)],
                            },
                            Action {
                                name: "startTimer".into(),
                                args: vec![],
                            },
                        ],
                        description: Some("Initiate payment".into()),
                        delay_ms: None,
                        targets: vec![],
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec!["initial".into()],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
                StateNode {
                    name: "processing".into(),
                    description: None,
                    initial: Some("auth".into()),
                    entry: vec!["logProcessing".into()],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![
                        StateNode {
                            name: "auth".into(),
                            description: None,
                            initial: None,
                            entry: vec![],
                            exit: vec![],
                            transitions: vec![Transition {
                                event: "AUTH_OK".into(),
                                target: Some("capture".into()),
                                guard: None,
                                actions: vec![Action {
                                    name: "assign".into(),
                                    args: vec![
                                        serde_json::json!("authId"),
                                        serde_json::json!("abc-123"),
                                    ],
                                }],
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
                            name: "capture".into(),
                            description: None,
                            initial: None,
                            entry: vec!["capturePayment".into()],
                            exit: vec![],
                            transitions: vec![Transition {
                                event: "CAPTURE_OK".into(),
                                target: Some("done".into()),
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
                    ],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
                StateNode {
                    name: "done".into(),
                    description: Some("Transaction complete".into()),
                    initial: None,
                    entry: vec!["notifySuccess".into()],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Final,
                    tags: vec!["terminal".into()],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
            ],

            setup: None,
        };
        let json = serde_json::to_string_pretty(&machine).unwrap();
        let round_tripped: Machine = serde_json::from_str(&json).unwrap();
        assert_eq!(machine, round_tripped);
    }
}
