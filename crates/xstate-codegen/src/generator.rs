use crate::error::CodegenError;
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use std::collections::BTreeSet;
use xstate_ir::machine::{ContextField, Machine};
use xstate_ir::state::StateNode;

pub fn generate(machine: &Machine) -> Result<TokenStream, CodegenError> {
    let state_enum = generate_state_enum(machine);
    let event_enum = generate_event_enum(machine);
    let event_payload = generate_event_payload_type();
    let context_struct = generate_context_struct(machine)?;
    let machine_impl = generate_machine_impl(machine)?;

    let tokens = quote! {
        #state_enum
        #event_enum
        #event_payload
        #context_struct
        #machine_impl
    };

    Ok(tokens)
}

fn generate_state_enum(machine: &Machine) -> TokenStream {
    let all_states = collect_all_states(&machine.states);
    let variants: Vec<_> = all_states
        .iter()
        .map(|s| {
            let name = to_pascal_case(&s.name);
            let ident = format_ident!("{}", name);
            quote! { #ident }
        })
        .collect();

    quote! {
        #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
        pub enum State {
            #(#variants),*
        }
    }
}

fn generate_event_enum(machine: &Machine) -> TokenStream {
    let all_states = collect_all_states(&machine.states);
    let mut event_names = BTreeSet::new();
    for state in &all_states {
        for transition in &state.transitions {
            event_names.insert(transition.event.clone());
        }
    }

    let variants: Vec<_> = event_names
        .iter()
        .map(|name| {
            let ident = format_ident!("{}", to_pascal_case(name));
            quote! { #ident }
        })
        .collect();

    if variants.is_empty() {
        return quote! {
            #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
            pub enum Event {}
        };
    }

    quote! {
        #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
        pub enum Event {
            #(#variants),*
        }
    }
}

fn generate_event_payload_type() -> TokenStream {
    quote! {
        pub type EventPayload = ::serde_json::Value;
    }
}

fn generate_context_struct(machine: &Machine) -> Result<TokenStream, CodegenError> {
    let fields: Result<Vec<_>, _> = machine
        .context
        .fields
        .iter()
        .map(|field| {
            let name = format_ident!("{}", field.name);
            let rust_type = type_name_to_rust_tokens(&field.type_name)?;
            Ok(quote! {
                pub #name: #rust_type
            })
        })
        .collect();
    let fields = fields?;

    Ok(quote! {
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
        pub struct Context {
            #(#fields),*
        }
    })
}

fn generate_machine_impl(machine: &Machine) -> Result<TokenStream, CodegenError> {
    let all_states = collect_all_states(&machine.states);
    let mut arms: Vec<TokenStream> = Vec::new();

    for state in &all_states {
        let state_variant = format_ident!("{}", to_pascal_case(&state.name));
        for transition in &state.transitions {
            let event_variant = format_ident!("{}", to_pascal_case(&transition.event));
            let target = match &transition.target {
                Some(target_name) => {
                    let target_variant = format_ident!("{}", to_pascal_case(target_name));
                    quote! { State::#target_variant }
                }
                None => {
                    quote! { state.clone() }
                }
            };
            arms.push(quote! {
                (State::#state_variant, Event::#event_variant) => #target,
            });
        }
    }

    let initial_variant = format_ident!("{}", to_pascal_case(&machine.initial));

    let context_fields: Result<Vec<_>, _> = machine
        .context
        .fields
        .iter()
        .map(|field| {
            let name = format_ident!("{}", field.name);
            let default_val = default_value_to_tokens(field)?;
            Ok(quote! { #name: #default_val })
        })
        .collect();
    let context_fields = context_fields?;

    Ok(quote! {
        pub struct MachineImpl;

        impl ::xstate_core::Machine for MachineImpl {
            type State = State;
            type Event = Event;
            type EventPayload = EventPayload;
            type Context = Context;

            fn initial_state(&self) -> Self::State {
                State::#initial_variant
            }

            fn initial_context(&self) -> Self::Context {
                Context {
                    #(#context_fields),*
                }
            }

            fn transition(
                &self,
                state: &Self::State,
                event: &Self::Event,
                _payload: &Self::EventPayload,
                _context: &mut Self::Context,
            ) -> Self::State {
                match (state, event) {
                    #(#arms)*
                    _ => state.clone(),
                }
            }
        }
    })
}

fn to_pascal_case(s: &str) -> String {
    s.split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let mut word = String::new();
                    word.push(first.to_ascii_uppercase());
                    word.extend(chars.map(|c| c.to_ascii_lowercase()));
                    word
                }
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn collect_all_states(states: &[StateNode]) -> Vec<&StateNode> {
    let mut result = Vec::new();
    for state in states {
        result.push(state);
        let children = collect_all_states(&state.children);
        result.extend(children);
    }
    result
}

fn type_name_to_rust_tokens(type_name: &str) -> Result<TokenStream, CodegenError> {
    match type_name {
        "number" => Ok(quote! { i64 }),
        "string" => Ok(quote! { String }),
        "boolean" => Ok(quote! { bool }),
        other => Err(CodegenError::UnknownType(other.to_string())),
    }
}

fn default_value_to_tokens(field: &ContextField) -> Result<TokenStream, CodegenError> {
    match field.type_name.as_str() {
        "number" => {
            let val = field.default_value.as_i64().unwrap_or(0);
            let lit = Literal::i64_suffixed(val);
            Ok(quote! { #lit })
        }
        "string" => {
            let val = field.default_value.as_str().unwrap_or("").to_string();
            Ok(quote! { #val })
        }
        "boolean" => {
            let val = field.default_value.as_bool().unwrap_or(false);
            Ok(quote! { #val })
        }
        other => Err(CodegenError::UnknownType(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xstate_ir::machine::ContextSchema;
    use xstate_ir::state::StateType;
    use xstate_ir::transition::Transition;

    #[test]
    fn to_pascal_case_snake() {
        assert_eq!(to_pascal_case("inactive"), "Inactive");
    }

    #[test]
    fn to_pascal_case_screaming_snake() {
        assert_eq!(to_pascal_case("TOGGLE_BOLD"), "ToggleBold");
    }

    #[test]
    fn to_pascal_case_with_dashes() {
        assert_eq!(to_pascal_case("some-event"), "SomeEvent");
    }

    #[test]
    fn to_pascal_case_single_word() {
        assert_eq!(to_pascal_case("toggle"), "Toggle");
    }

    #[test]
    fn to_pascal_case_empty() {
        assert_eq!(to_pascal_case(""), "");
    }

    #[test]
    fn generate_toggle_machine() {
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
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
            ],
        };

        let tokens = generate(&machine).unwrap();
        let output = tokens.to_string();

        assert!(output.contains("Inactive"));
        assert!(output.contains("Active"));
        assert!(output.contains("Toggle"));
        assert!(output.contains("pub count"));
        assert!(output.contains("i64"));
        assert!(output.contains("State :: Inactive"));
        assert!(output.contains("State :: Active"));
        assert!(output.contains("Event :: Toggle"));
        assert!(output.contains("0i64"));
        let compact = output.replace(' ', "");
        assert!(compact.contains("pubenumState{"));
        assert!(compact.contains("pubenumEvent{"));
        assert!(compact.contains("pubstructMachineImpl;"));
        assert!(compact.contains("impl::xstate_core::MachineforMachineImpl"));
        assert!(compact.contains("(State::Inactive,Event::Toggle)=>State::Active"));
        assert!(compact.contains("(State::Active,Event::Toggle)=>State::Inactive"));
        assert!(output.contains("type EventPayload = :: serde_json :: Value"));
        let compact = output.replace(' ', "");
        assert!(compact.contains("_payload:&Self::EventPayload"));
        assert!(compact.contains("typeEventPayload=EventPayload"));
    }

    #[test]
    fn generate_compound_state_machine() {
        let machine = Machine {
            id: "trafficLight".into(),
            initial: "red".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![StateNode {
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
            }],
        };

        let tokens = generate(&machine).unwrap();
        let output = tokens.to_string();

        assert!(output.contains("Red"));
        assert!(output.contains("Walk"));
        assert!(output.contains("Stop"));
        assert!(output.contains("Timer"));
        assert!(output.contains("Reset"));
        let compact = output.replace(' ', "");
        assert!(compact.contains("(State::Red,Event::Timer)=>State::Green"));
        assert!(compact.contains("(State::Walk,Event::Reset)=>State::Stop"));
    }

    #[test]
    fn generate_empty_context() {
        let machine = Machine {
            id: "noContext".into(),
            initial: "idle".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![StateNode {
                name: "idle".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            }],
        };

        let tokens = generate(&machine).unwrap();
        let output = tokens.to_string();

        assert!(output.contains("pub struct Context"));
        let compact = output.replace(' ', "");
        assert!(compact.contains("impl::xstate_core::MachineforMachineImpl"));
    }

    #[test]
    fn generate_context_with_multiple_fields() {
        let machine = Machine {
            id: "multiField".into(),
            initial: "start".into(),
            context: ContextSchema {
                fields: vec![
                    ContextField {
                        name: "count".into(),
                        type_name: "number".into(),
                        default_value: serde_json::json!(0),
                    },
                    ContextField {
                        name: "name".into(),
                        type_name: "string".into(),
                        default_value: serde_json::json!("hello"),
                    },
                    ContextField {
                        name: "enabled".into(),
                        type_name: "boolean".into(),
                        default_value: serde_json::json!(true),
                    },
                ],
            },
            states: vec![StateNode {
                name: "start".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            }],
        };

        let tokens = generate(&machine).unwrap();
        let output = tokens.to_string();

        assert!(output.contains("i64"));
        assert!(output.contains("String"));
        assert!(output.contains("bool"));
        let compact = output.replace(' ', "");
        assert!(compact.contains("pubcount:i64"));
        assert!(compact.contains("pubname:String"));
        assert!(compact.contains("pubenabled:bool"));
        assert!(compact.contains("count:0i64"));
        assert!(compact.contains("name:\"hello\""));
        assert!(compact.contains("enabled:true"));
    }

    #[test]
    fn unknown_type_error() {
        let machine = Machine {
            id: "bad".into(),
            initial: "x".into(),
            context: ContextSchema {
                fields: vec![ContextField {
                    name: "val".into(),
                    type_name: "object".into(),
                    default_value: serde_json::json!({}),
                }],
            },
            states: vec![StateNode {
                name: "x".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            }],
        };

        let result = generate(&machine);
        assert!(result.is_err());
    }

    #[test]
    fn transition_without_target_self_transition() {
        let machine = Machine {
            id: "selfTransition".into(),
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
                    actions: vec![],
                    description: None,
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            }],
        };

        let tokens = generate(&machine).unwrap();
        let output = tokens.to_string();

        let compact = output.replace(' ', "");
        assert!(compact.contains("(State::Idle,Event::Log)=>state.clone()"));
    }

    #[test]
    fn rounds_to_valid_rust_syntax() {
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
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
            ],
        };

        let tokens = generate(&machine).unwrap();
        let output = tokens.to_string();

        let file: syn::File = syn::parse_str(&output).expect("Generated code should be valid Rust");
        assert!(!file.items.is_empty());
    }
}
