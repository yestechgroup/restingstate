use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use xstate_ir::actors::SetupConfig;

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

pub fn generate_guard_modules(setup: &SetupConfig) -> TokenStream {
    if setup.guards.is_empty() {
        return quote! {};
    }

    let mut sorted_guards: Vec<_> = setup.guards.iter().collect();
    sorted_guards.sort_by_key(|(name, _)| name.to_string());

    let fns: Vec<_> = sorted_guards
        .iter()
        .map(|(name, _)| {
            let fn_name = format_ident!("{}", to_snake_case(name));
            let todo_msg = format!("Implement guard: {}", name);
            quote! {
                pub fn #fn_name(_ctx: &::serde_json::Value, _event: &::serde_json::Value) -> bool {
                    todo!(#todo_msg)
                }
            }
        })
        .collect();

    quote! {
        pub mod guards {
            #(#fns)*
        }
    }
}

pub fn generate_action_modules(setup: &SetupConfig) -> TokenStream {
    if setup.actions.is_empty() {
        return quote! {};
    }

    let mut sorted_actions: Vec<_> = setup.actions.iter().collect();
    sorted_actions.sort_by_key(|(name, _)| name.to_string());

    let fns: Vec<_> = sorted_actions
        .iter()
        .map(|(name, _)| {
            let fn_name = format_ident!("{}", to_snake_case(name));
            let todo_msg = format!("Implement action: {}", name);
            quote! {
                pub fn #fn_name(_args: &[::serde_json::Value], _event: &::serde_json::Value, _ctx: &mut ::serde_json::Value) {
                    todo!(#todo_msg)
                }
            }
        })
        .collect();

    quote! {
        pub mod actions {
            #(#fns)*
        }
    }
}

pub fn generate_actor_modules(setup: &SetupConfig) -> TokenStream {
    if setup.actors.is_empty() {
        return quote! {};
    }

    let mut sorted_actors: Vec<_> = setup.actors.iter().collect();
    sorted_actors.sort_by_key(|(name, _)| name.to_string());

    let fns: Vec<_> = sorted_actors
        .iter()
        .map(|(name, _)| {
            let fn_name = format_ident!("load_{}", to_snake_case(name));
            let todo_msg = format!("Implement actor: {}", name);
            quote! {
                pub fn #fn_name() -> impl ::xstate_core::ActorLogic {
                    todo!(#todo_msg)
                }
            }
        })
        .collect();

    quote! {
        pub mod actors {
            #(#fns)*
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use xstate_ir::actors::{ActionDef, ActorDef, GuardDef};

    #[test]
    fn empty_setup_generates_nothing() {
        let setup = SetupConfig::default();
        let guards = generate_guard_modules(&setup);
        let actions = generate_action_modules(&setup);
        let actors = generate_actor_modules(&setup);
        assert!(guards.is_empty());
        assert!(actions.is_empty());
        assert!(actors.is_empty());
    }

    #[test]
    fn single_guard_module() {
        let mut guards = HashMap::new();
        guards.insert(
            "isValid".into(),
            GuardDef {
                name: "isValid".into(),
                params: vec![],
            },
        );
        let setup = SetupConfig {
            actors: HashMap::new(),
            guards,
            actions: HashMap::new(),
            delays: HashMap::new(),
        };
        let tokens = generate_guard_modules(&setup);
        let code = tokens.to_string();
        assert!(
            code.contains("pub mod guards"),
            "missing guards module: {code}"
        );
        assert!(code.contains("fn is_valid"), "missing is_valid fn: {code}");
        assert!(code.contains("todo !"), "missing todo: {code}");
    }

    #[test]
    fn single_action_module() {
        let mut actions = HashMap::new();
        actions.insert(
            "increment".into(),
            ActionDef {
                name: "increment".into(),
                params: vec![],
            },
        );
        let setup = SetupConfig {
            actors: HashMap::new(),
            guards: HashMap::new(),
            actions,
            delays: HashMap::new(),
        };
        let tokens = generate_action_modules(&setup);
        let code = tokens.to_string();
        assert!(
            code.contains("pub mod actions"),
            "missing actions module: {code}"
        );
        assert!(
            code.contains("fn increment"),
            "missing increment fn: {code}"
        );
        assert!(code.contains("todo !"), "missing todo: {code}");
    }

    #[test]
    fn single_actor_module() {
        let mut actors = HashMap::new();
        actors.insert(
            "loader".into(),
            ActorDef::FromPromise {
                src: "loader".into(),
            },
        );
        let setup = SetupConfig {
            actors,
            guards: HashMap::new(),
            actions: HashMap::new(),
            delays: HashMap::new(),
        };
        let tokens = generate_actor_modules(&setup);
        let code = tokens.to_string();
        assert!(
            code.contains("pub mod actors"),
            "missing actors module: {code}"
        );
        assert!(
            code.contains("fn load_loader"),
            "missing load_loader fn: {code}"
        );
        assert!(code.contains("ActorLogic"), "missing ActorLogic: {code}");
        assert!(code.contains("todo !"), "missing todo: {code}");
    }
}
