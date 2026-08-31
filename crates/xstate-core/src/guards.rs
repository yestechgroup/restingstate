use std::collections::HashMap;

/// Callback signature for guard evaluation: `(context, event) -> bool`.
type GuardFn = dyn Fn(&serde_json::Value, &serde_json::Value) -> bool + Send + Sync;

/// A named, composable guard.
pub struct GuardImpl {
    pub name: String,
    pub evaluate: Box<GuardFn>,
}

impl GuardImpl {
    pub fn new(
        name: impl Into<String>,
        f: impl Fn(&serde_json::Value, &serde_json::Value) -> bool + Send + Sync + 'static,
    ) -> Self {
        GuardImpl {
            name: name.into(),
            evaluate: Box::new(f),
        }
    }

    pub fn always_true(name: impl Into<String>) -> Self {
        Self::new(name, |_, _| true)
    }
}

/// Registry of named guards.
pub struct GuardRegistry {
    guards: HashMap<String, GuardImpl>,
}

impl GuardRegistry {
    pub fn new() -> Self {
        GuardRegistry {
            guards: HashMap::new(),
        }
    }

    pub fn register(&mut self, guard: GuardImpl) {
        self.guards.insert(guard.name.clone(), guard);
    }

    pub fn evaluate(
        &self,
        name: &str,
        context: &serde_json::Value,
        event: &serde_json::Value,
    ) -> bool {
        match self.guards.get(name) {
            Some(guard) => (guard.evaluate)(context, event),
            None => true,
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.guards.contains_key(name)
    }
}

impl Default for GuardRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Combine guards with AND logic.
pub fn and(guards: Vec<GuardImpl>) -> GuardImpl {
    let names: Vec<String> = guards.iter().map(|g| g.name.clone()).collect();
    let name = names.join(" AND ");
    GuardImpl::new(name, move |ctx, event| {
        guards.iter().all(|g| (g.evaluate)(ctx, event))
    })
}

/// Combine guards with OR logic.
pub fn or(guards: Vec<GuardImpl>) -> GuardImpl {
    let names: Vec<String> = guards.iter().map(|g| g.name.clone()).collect();
    let name = names.join(" OR ");
    GuardImpl::new(name, move |ctx, event| {
        guards.iter().any(|g| (g.evaluate)(ctx, event))
    })
}

/// Negate a guard.
pub fn not(guard: GuardImpl) -> GuardImpl {
    let name = format!("NOT {}", guard.name);
    GuardImpl::new(name, move |ctx, event| !(guard.evaluate)(ctx, event))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_always_true() {
        let g = GuardImpl::always_true("yes");
        assert!((g.evaluate)(
            &serde_json::Value::Null,
            &serde_json::Value::Null
        ));
    }

    #[test]
    fn guard_custom() {
        let g = GuardImpl::new("check", |ctx, _| ctx["count"].as_i64().unwrap_or(0) > 5);
        let ctx = serde_json::json!({"count": 10});
        assert!((g.evaluate)(&ctx, &serde_json::Value::Null));
        let ctx2 = serde_json::json!({"count": 3});
        assert!(!(g.evaluate)(&ctx2, &serde_json::Value::Null));
    }

    #[test]
    fn registry_evaluate_existing() {
        let mut reg = GuardRegistry::new();
        reg.register(GuardImpl::new("positive", |ctx, _| {
            ctx["val"].as_i64().unwrap_or(0) > 0
        }));
        let ctx = serde_json::json!({"val": 5});
        assert!(reg.evaluate("positive", &ctx, &serde_json::Value::Null));
    }

    #[test]
    fn registry_evaluate_missing_defaults_true() {
        let reg = GuardRegistry::new();
        assert!(reg.evaluate(
            "nonexistent",
            &serde_json::Value::Null,
            &serde_json::Value::Null
        ));
    }

    #[test]
    fn registry_has() {
        let mut reg = GuardRegistry::new();
        assert!(!reg.has("foo"));
        reg.register(GuardImpl::always_true("foo"));
        assert!(reg.has("foo"));
    }

    #[test]
    fn and_combines_guards() {
        let g1 = GuardImpl::new("a", |ctx, _| ctx["a"].as_bool().unwrap_or(false));
        let g2 = GuardImpl::new("b", |ctx, _| ctx["b"].as_bool().unwrap_or(false));
        let combined = and(vec![g1, g2]);
        assert!((combined.evaluate)(
            &serde_json::json!({"a": true, "b": true}),
            &serde_json::Value::Null
        ));
        assert!(!(combined.evaluate)(
            &serde_json::json!({"a": true, "b": false}),
            &serde_json::Value::Null
        ));
    }

    #[test]
    fn or_combines_guards() {
        let g1 = GuardImpl::new("a", |ctx, _| ctx["a"].as_bool().unwrap_or(false));
        let g2 = GuardImpl::new("b", |ctx, _| ctx["b"].as_bool().unwrap_or(false));
        let combined = or(vec![g1, g2]);
        assert!((combined.evaluate)(
            &serde_json::json!({"a": false, "b": true}),
            &serde_json::Value::Null
        ));
        assert!(!(combined.evaluate)(
            &serde_json::json!({"a": false, "b": false}),
            &serde_json::Value::Null
        ));
    }

    #[test]
    fn not_negates_guard() {
        let g = GuardImpl::new("yes", |_, _| true);
        let negated = not(g);
        assert!(!(negated.evaluate)(
            &serde_json::Value::Null,
            &serde_json::Value::Null
        ));
    }
}
