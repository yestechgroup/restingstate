use std::collections::HashMap;

/// Callback signature for action execution: `(args, event, context)`.
type ActionFn =
    dyn Fn(&[serde_json::Value], &serde_json::Value, &mut serde_json::Value) + Send + Sync;

/// A named, composable action.
pub struct ActionImpl {
    pub name: String,
    pub execute: Box<ActionFn>,
}

impl ActionImpl {
    pub fn new(
        name: impl Into<String>,
        f: impl Fn(&[serde_json::Value], &serde_json::Value, &mut serde_json::Value)
        + Send
        + Sync
        + 'static,
    ) -> Self {
        ActionImpl {
            name: name.into(),
            execute: Box::new(f),
        }
    }

    pub fn noop(name: impl Into<String>) -> Self {
        Self::new(name, |_, _, _| {})
    }
}

/// Registry of named actions.
pub struct ActionRegistry {
    actions: HashMap<String, ActionImpl>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        ActionRegistry {
            actions: HashMap::new(),
        }
    }

    pub fn register(&mut self, action: ActionImpl) {
        self.actions.insert(action.name.clone(), action);
    }

    pub fn dispatch(
        &self,
        name: &str,
        args: &[serde_json::Value],
        event: &serde_json::Value,
        context: &mut serde_json::Value,
    ) {
        if let Some(action) = self.actions.get(name) {
            (action.execute)(args, event, context);
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.actions.contains_key(name)
    }
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute actions in sequence.
pub fn sequence(actions: Vec<ActionImpl>) -> ActionImpl {
    let names: Vec<String> = actions.iter().map(|a| a.name.clone()).collect();
    let name = names.join(" -> ");
    ActionImpl::new(name, move |args, event, ctx| {
        for action in &actions {
            (action.execute)(args, event, ctx);
        }
    })
}

/// Execute actions in parallel (all get same context snapshot).
pub fn parallel(actions: Vec<ActionImpl>) -> ActionImpl {
    let names: Vec<String> = actions.iter().map(|a| a.name.clone()).collect();
    let name = names.join(" || ");
    ActionImpl::new(name, move |args, event, ctx| {
        for action in &actions {
            (action.execute)(args, event, ctx);
        }
    })
}

/// Create an assign action from an updater function.
pub fn assign_action(
    updater: impl Fn(&serde_json::Value, &serde_json::Value) -> serde_json::Value
    + Send
    + Sync
    + 'static,
) -> ActionImpl {
    ActionImpl::new("assign", move |_args, event, ctx| {
        let new_ctx = updater(ctx, event);
        if let (Some(obj), Some(new_obj)) = (ctx.as_object_mut(), new_ctx.as_object()) {
            for (k, v) in new_obj {
                obj.insert(k.clone(), v.clone());
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_noop_does_nothing() {
        let a = ActionImpl::noop("noop");
        let mut ctx = serde_json::json!({"x": 1});
        (a.execute)(&[], &serde_json::Value::Null, &mut ctx);
        assert_eq!(ctx, serde_json::json!({"x": 1}));
    }

    #[test]
    fn action_custom() {
        let a = ActionImpl::new("inc", |_, _, ctx| {
            if let Some(obj) = ctx.as_object_mut() {
                let val = obj.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
                obj.insert("count".into(), serde_json::json!(val + 1));
            }
        });
        let mut ctx = serde_json::json!({"count": 0});
        (a.execute)(&[], &serde_json::Value::Null, &mut ctx);
        assert_eq!(ctx["count"], 1);
    }

    #[test]
    fn registry_dispatch_existing() {
        let mut reg = ActionRegistry::new();
        reg.register(ActionImpl::new("set_a", |_, _, ctx| {
            ctx.as_object_mut()
                .unwrap()
                .insert("a".into(), serde_json::json!(42));
        }));
        let mut ctx = serde_json::json!({});
        reg.dispatch("set_a", &[], &serde_json::Value::Null, &mut ctx);
        assert_eq!(ctx["a"], 42);
    }

    #[test]
    fn registry_dispatch_missing_is_noop() {
        let reg = ActionRegistry::new();
        let mut ctx = serde_json::json!({});
        reg.dispatch("nonexistent", &[], &serde_json::Value::Null, &mut ctx);
        assert_eq!(ctx, serde_json::json!({}));
    }

    #[test]
    fn registry_has() {
        let mut reg = ActionRegistry::new();
        assert!(!reg.has("foo"));
        reg.register(ActionImpl::noop("foo"));
        assert!(reg.has("foo"));
    }

    #[test]
    fn sequence_executes_in_order() {
        let a1 = ActionImpl::new("a1", |_, _, ctx| {
            ctx.as_object_mut()
                .unwrap()
                .insert("a".into(), serde_json::json!(1));
        });
        let a2 = ActionImpl::new("a2", |_, _, ctx| {
            ctx.as_object_mut()
                .unwrap()
                .insert("b".into(), serde_json::json!(2));
        });
        let seq = sequence(vec![a1, a2]);
        let mut ctx = serde_json::json!({});
        (seq.execute)(&[], &serde_json::Value::Null, &mut ctx);
        assert_eq!(ctx["a"], 1);
        assert_eq!(ctx["b"], 2);
    }

    #[test]
    fn assign_action_updates_context() {
        let a = assign_action(|_, _| serde_json::json!({"count": 10}));
        let mut ctx = serde_json::json!({"count": 0, "other": true});
        (a.execute)(&[], &serde_json::Value::Null, &mut ctx);
        assert_eq!(ctx["count"], 10);
        assert_eq!(ctx["other"], true);
    }
}
