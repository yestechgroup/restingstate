use std::collections::HashMap;
use std::fmt;

/// Error produced when a fallible guard fails to evaluate.
#[derive(Debug)]
pub struct GuardError {
    /// Name of the guard that errored.
    pub guard_name: String,
    /// The underlying error.
    pub source: Box<dyn std::error::Error + Send + Sync>,
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "guard '{}' evaluation failed: {}",
            self.guard_name, self.source
        )
    }
}

impl std::error::Error for GuardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Callback signature for total guard evaluation: `(context, event) -> bool`.
type GuardFn = dyn Fn(&serde_json::Value, &serde_json::Value) -> bool + Send + Sync;

/// Callback signature for fallible guard evaluation:
/// `(context, event) -> Result<bool, Box<dyn Error>>`.
type FallibleGuardFn = dyn Fn(
        &serde_json::Value,
        &serde_json::Value,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>
    + Send
    + Sync;

enum GuardFnKind {
    Total(Box<GuardFn>),
    Fallible(Box<FallibleGuardFn>),
}

/// A named, composable guard.
pub struct GuardImpl {
    pub name: String,
    kind: GuardFnKind,
}

impl GuardImpl {
    pub fn new(
        name: impl Into<String>,
        f: impl Fn(&serde_json::Value, &serde_json::Value) -> bool + Send + Sync + 'static,
    ) -> Self {
        GuardImpl {
            name: name.into(),
            kind: GuardFnKind::Total(Box::new(f)),
        }
    }

    /// Create a fallible guard that can return `Result<bool, Error>`.
    pub fn new_fallible(
        name: impl Into<String>,
        f: impl Fn(
            &serde_json::Value,
            &serde_json::Value,
        ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        GuardImpl {
            name: name.into(),
            kind: GuardFnKind::Fallible(Box::new(f)),
        }
    }

    pub fn always_true(name: impl Into<String>) -> Self {
        Self::new(name, |_, _| true)
    }

    /// Evaluate this guard using the total (infallible) path.
    /// For fallible guards, `Err` is mapped to `true` (backward compat).
    pub fn evaluate_total(&self, context: &serde_json::Value, event: &serde_json::Value) -> bool {
        match &self.kind {
            GuardFnKind::Total(f) => f(context, event),
            GuardFnKind::Fallible(f) => f(context, event).unwrap_or(true),
        }
    }

    /// Evaluate this guard using the fallible path.
    /// For total guards, always returns `Ok(bool)`.
    pub fn evaluate_fallible(
        &self,
        context: &serde_json::Value,
        event: &serde_json::Value,
    ) -> Result<bool, GuardError> {
        match &self.kind {
            GuardFnKind::Total(f) => Ok(f(context, event)),
            GuardFnKind::Fallible(f) => f(context, event).map_err(|source| GuardError {
                guard_name: self.name.clone(),
                source,
            }),
        }
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

    /// Total (infallible) evaluation — backward compatible.
    /// Fallible guards have `Err` mapped to `true`.
    pub fn evaluate(
        &self,
        name: &str,
        context: &serde_json::Value,
        event: &serde_json::Value,
    ) -> bool {
        match self.guards.get(name) {
            Some(guard) => guard.evaluate_total(context, event),
            None => true,
        }
    }

    /// Fallible evaluation — preserves guard errors.
    /// Missing guards default to `Ok(true)`.
    pub fn evaluate_fallible(
        &self,
        name: &str,
        context: &serde_json::Value,
        event: &serde_json::Value,
    ) -> Result<bool, GuardError> {
        match self.guards.get(name) {
            Some(guard) => guard.evaluate_fallible(context, event),
            None => Ok(true),
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

/// Combine guards with AND logic (total path).
pub fn and(guards: Vec<GuardImpl>) -> GuardImpl {
    let names: Vec<String> = guards.iter().map(|g| g.name.clone()).collect();
    let name = names.join(" AND ");
    GuardImpl::new(name, move |ctx, event| {
        guards.iter().all(|g| g.evaluate_total(ctx, event))
    })
}

/// Combine guards with OR logic (total path).
pub fn or(guards: Vec<GuardImpl>) -> GuardImpl {
    let names: Vec<String> = guards.iter().map(|g| g.name.clone()).collect();
    let name = names.join(" OR ");
    GuardImpl::new(name, move |ctx, event| {
        guards.iter().any(|g| g.evaluate_total(ctx, event))
    })
}

/// Negate a guard (total path).
pub fn not(guard: GuardImpl) -> GuardImpl {
    let name = format!("NOT {}", guard.name);
    GuardImpl::new(name, move |ctx, event| !guard.evaluate_total(ctx, event))
}

/// Combine fallible guards with AND logic.
/// Short-circuits on first `Ok(false)` or first `Err` (if no earlier `Ok(false)`).
pub fn and_fallible(guards: Vec<GuardImpl>) -> GuardImpl {
    let names: Vec<String> = guards.iter().map(|g| g.name.clone()).collect();
    let name = names.join(" AND ");
    GuardImpl::new_fallible(name, move |ctx, event| {
        let mut saw_deny = false;
        for g in &guards {
            match g.evaluate_fallible(ctx, event) {
                Ok(true) => {}
                Ok(false) => {
                    saw_deny = true;
                    break;
                }
                Err(e) => {
                    if saw_deny {
                        return Ok(false);
                    }
                    return Err(Box::new(e));
                }
            }
        }
        Ok(!saw_deny)
    })
}

/// Combine fallible guards with OR logic.
/// Short-circuits on first `Ok(true)`. Error propagated only if no guard resolved to `true`.
pub fn or_fallible(guards: Vec<GuardImpl>) -> GuardImpl {
    let names: Vec<String> = guards.iter().map(|g| g.name.clone()).collect();
    let name = names.join(" OR ");
    GuardImpl::new_fallible(name, move |ctx, event| {
        let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
        for g in &guards {
            match g.evaluate_fallible(ctx, event) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(e) => {
                    last_err = Some(Box::new(e));
                }
            }
        }
        match last_err {
            Some(e) => Err(e),
            None => Ok(false),
        }
    })
}

/// Negate a fallible guard. Error is propagated.
pub fn not_fallible(guard: GuardImpl) -> GuardImpl {
    let name = format!("NOT {}", guard.name);
    GuardImpl::new_fallible(
        name,
        move |ctx, event| -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
            guard
                .evaluate_fallible(ctx, event)
                .map(|b| !b)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        },
    )
}

/// The typed counterpart of [`GuardError`]: the guard name plus the
/// guard's OWN error type, preserved end-to-end instead of erased into
/// `Box<dyn Error>`. Produced by [`TypedGuard::evaluate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardFailure<E> {
    /// Name of the guard that failed.
    pub guard: String,
    /// The guard's typed error.
    pub source: E,
}

impl<E: fmt::Display> fmt::Display for GuardFailure<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "guard '{}' failed: {}", self.guard, self.source)
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<GuardFailure<E>> for GuardError {
    /// Bridge into the erased world: a typed failure can cross into any API
    /// that consumes [`GuardError`], with the typed error preserved in the
    /// `source` chain (downcast-friendly).
    fn from(f: GuardFailure<E>) -> Self {
        GuardError {
            guard_name: f.guard,
            source: Box::new(f.source),
        }
    }
}

/// The erased evaluation closure of a [`TypedGuard`].
type TypedGuardFn<C, Ev, E> = Box<dyn Fn(&C, &Ev) -> Result<bool, E> + Send + Sync>;

/// A guard with TYPED context, event, and error — for consumers whose
/// transition layer is generically typed (hand-written `Machine`
/// implementations, typed runtimes) and whose failure modes are enumerated
/// rather than erased.
///
/// `TypedGuard` deliberately does NOT live in a [`GuardRegistry`]: the
/// registry is JSON-erased by design (it serves the dynamic `Interpreter`
/// path), and erasing a typed error back into `Box<dyn Error>` would defeat
/// the point. Hold typed guards directly and call [`TypedGuard::evaluate`];
/// when one must cross into the erased world, [`GuardFailure`] converts
/// into [`GuardError`] via `From`.
///
/// ```
/// use core::fmt;
///
/// use xstate_core::guards::TypedGuard;
///
/// #[derive(Debug, PartialEq)]
/// enum AuctionError { NotEligible }
/// impl fmt::Display for AuctionError {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         match self { Self::NotEligible => write!(f, "participant not eligible") }
///     }
/// }
/// impl std::error::Error for AuctionError {}
///
/// let guard: TypedGuard<u64, (), AuctionError> =
///     TypedGuard::new("eligible", |deposit, _| {
///         if *deposit >= 5_000 { Ok(true) } else { Err(AuctionError::NotEligible) }
///     });
///
/// assert!(guard.evaluate(&5_000, &()).unwrap());
/// let failure = guard.evaluate(&100, &()).unwrap_err();
/// assert_eq!(failure.guard, "eligible");
/// assert_eq!(failure.source, AuctionError::NotEligible);
/// ```
pub struct TypedGuard<C, Ev, E> {
    name: String,
    f: TypedGuardFn<C, Ev, E>,
}

impl<C, Ev, E> TypedGuard<C, Ev, E> {
    /// Names and wraps a typed guard closure.
    pub fn new(
        name: impl Into<String>,
        f: impl Fn(&C, &Ev) -> Result<bool, E> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            f: Box::new(f),
        }
    }

    /// The guard's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Typed evaluation: the error survives as `E`, wrapped with the guard
    /// name in [`GuardFailure`].
    pub fn evaluate(&self, context: &C, event: &Ev) -> Result<bool, GuardFailure<E>> {
        (self.f)(context, event).map_err(|source| GuardFailure {
            guard: self.name.clone(),
            source,
        })
    }
}

impl<E: std::error::Error + Send + Sync + 'static>
    TypedGuard<serde_json::Value, serde_json::Value, E>
{
    /// Erase into a registry-compatible [`GuardImpl`]: the typed error is
    /// boxed at this boundary (and remains downcast-recoverable through
    /// `GuardError::source`). Use when a typed guard must live in a
    /// `GuardRegistry`; prefer direct [`TypedGuard::evaluate`] otherwise.
    pub fn erased(self) -> GuardImpl {
        GuardImpl::new_fallible(self.name, move |ctx, ev| {
            (self.f)(ctx, ev).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_always_true() {
        let g = GuardImpl::always_true("yes");
        assert!(g.evaluate_total(&serde_json::Value::Null, &serde_json::Value::Null));
    }

    #[test]
    fn guard_custom() {
        let g = GuardImpl::new("check", |ctx, _| ctx["count"].as_i64().unwrap_or(0) > 5);
        let ctx = serde_json::json!({"count": 10});
        assert!(g.evaluate_total(&ctx, &serde_json::Value::Null));
        let ctx2 = serde_json::json!({"count": 3});
        assert!(!g.evaluate_total(&ctx2, &serde_json::Value::Null));
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
        assert!(combined.evaluate_total(
            &serde_json::json!({"a": true, "b": true}),
            &serde_json::Value::Null
        ));
        assert!(!combined.evaluate_total(
            &serde_json::json!({"a": true, "b": false}),
            &serde_json::Value::Null
        ));
    }

    #[test]
    fn or_combines_guards() {
        let g1 = GuardImpl::new("a", |ctx, _| ctx["a"].as_bool().unwrap_or(false));
        let g2 = GuardImpl::new("b", |ctx, _| ctx["b"].as_bool().unwrap_or(false));
        let combined = or(vec![g1, g2]);
        assert!(combined.evaluate_total(
            &serde_json::json!({"a": false, "b": true}),
            &serde_json::Value::Null
        ));
        assert!(!combined.evaluate_total(
            &serde_json::json!({"a": false, "b": false}),
            &serde_json::Value::Null
        ));
    }

    #[test]
    fn not_negates_guard() {
        let g = GuardImpl::new("yes", |_, _| true);
        let negated = not(g);
        assert!(!negated.evaluate_total(&serde_json::Value::Null, &serde_json::Value::Null));
    }

    // --- Fallible guard tests ---

    #[test]
    fn guard_error_display() {
        let err = GuardError {
            guard_name: "myGuard".into(),
            source: "something went wrong".into(),
        };
        assert_eq!(
            err.to_string(),
            "guard 'myGuard' evaluation failed: something went wrong"
        );
    }

    #[test]
    fn fallible_guard_ok_true() {
        let g = GuardImpl::new_fallible("ok_guard", |_, _| Ok(true));
        assert!(
            g.evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn fallible_guard_ok_false() {
        let g = GuardImpl::new_fallible("deny_guard", |_, _| Ok(false));
        assert!(
            !g.evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn fallible_guard_err() {
        let g = GuardImpl::new_fallible("fail_guard", |_, _| Err("evaluation error".into()));
        let result = g.evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.guard_name, "fail_guard");
    }

    #[test]
    fn fallible_guard_err_total_path_maps_to_true() {
        let g = GuardImpl::new_fallible("fail_guard", |_, _| Err("evaluation error".into()));
        // Total path: Err → true (backward compat)
        assert!(g.evaluate_total(&serde_json::Value::Null, &serde_json::Value::Null));
    }

    #[test]
    fn registry_evaluate_fallible_existing() {
        let mut reg = GuardRegistry::new();
        reg.register(GuardImpl::new_fallible("positive", |ctx, _| {
            let val = ctx["val"].as_i64().unwrap_or(0);
            Ok(val > 0)
        }));
        let ctx = serde_json::json!({"val": 5});
        assert!(
            reg.evaluate_fallible("positive", &ctx, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn registry_evaluate_fallible_err() {
        let mut reg = GuardRegistry::new();
        reg.register(GuardImpl::new_fallible(
            "explode",
            |_, _| Err("boom".into()),
        ));
        let result = reg.evaluate_fallible(
            "explode",
            &serde_json::Value::Null,
            &serde_json::Value::Null,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().guard_name, "explode");
    }

    #[test]
    fn registry_evaluate_fallible_missing_defaults_true() {
        let reg = GuardRegistry::new();
        assert!(
            reg.evaluate_fallible(
                "nonexistent",
                &serde_json::Value::Null,
                &serde_json::Value::Null
            )
            .unwrap()
        );
    }

    #[test]
    fn registry_evaluate_total_with_fallible_guard_err_maps_to_true() {
        let mut reg = GuardRegistry::new();
        reg.register(GuardImpl::new_fallible("fail_guard", |_, _| {
            Err("boom".into())
        }));
        // Total path: Err → true
        assert!(reg.evaluate(
            "fail_guard",
            &serde_json::Value::Null,
            &serde_json::Value::Null
        ));
    }

    #[test]
    fn registry_mixed_total_and_fallible() {
        let mut reg = GuardRegistry::new();
        reg.register(GuardImpl::new("total", |ctx, _| {
            ctx["ok"].as_bool().unwrap_or(false)
        }));
        reg.register(GuardImpl::new_fallible("fallible", |ctx, _| {
            Ok(ctx["ok"].as_bool().unwrap_or(false))
        }));
        let ctx = serde_json::json!({"ok": true});
        assert!(
            reg.evaluate_fallible("total", &ctx, &serde_json::Value::Null)
                .unwrap()
        );
        assert!(
            reg.evaluate_fallible("fallible", &ctx, &serde_json::Value::Null)
                .unwrap()
        );
    }

    // --- Combinator tests with fallible guards ---

    #[test]
    fn and_fallible_both_allow() {
        let g1 = GuardImpl::new_fallible("a", |_, _| Ok(true));
        let g2 = GuardImpl::new_fallible("b", |_, _| Ok(true));
        let combined = and_fallible(vec![g1, g2]);
        assert!(
            combined
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn and_fallible_one_denies() {
        let g1 = GuardImpl::new_fallible("a", |_, _| Ok(true));
        let g2 = GuardImpl::new_fallible("b", |_, _| Ok(false));
        let combined = and_fallible(vec![g1, g2]);
        assert!(
            !combined
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn and_fallible_error_short_circuits() {
        let g1 = GuardImpl::new_fallible("a", |_, _| Ok(false));
        let g2 = GuardImpl::new_fallible("b", |_, _| Err("boom".into()));
        let combined = and_fallible(vec![g1, g2]);
        // Ok(false) seen before Err → Ok(false)
        assert!(
            !combined
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn and_fallible_error_propagates() {
        let g1 = GuardImpl::new_fallible("a", |_, _| Ok(true));
        let g2 = GuardImpl::new_fallible("b", |_, _| Err("boom".into()));
        let combined = and_fallible(vec![g1, g2]);
        // No Ok(false) seen → Err propagated
        assert!(
            combined
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .is_err()
        );
    }

    #[test]
    fn or_fallible_one_allows() {
        let g1 = GuardImpl::new_fallible("a", |_, _| Ok(false));
        let g2 = GuardImpl::new_fallible("b", |_, _| Ok(true));
        let combined = or_fallible(vec![g1, g2]);
        assert!(
            combined
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn or_fallible_error_ignored_if_true() {
        let g1 = GuardImpl::new_fallible("a", |_, _| Ok(true));
        let g2 = GuardImpl::new_fallible("b", |_, _| Err("boom".into()));
        let combined = or_fallible(vec![g1, g2]);
        assert!(
            combined
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn or_fallible_error_propagates() {
        let g1 = GuardImpl::new_fallible("a", |_, _| Ok(false));
        let g2 = GuardImpl::new_fallible("b", |_, _| Err("boom".into()));
        let combined = or_fallible(vec![g1, g2]);
        assert!(
            combined
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .is_err()
        );
    }

    #[test]
    fn not_fallible_allow() {
        let g = GuardImpl::new_fallible("yes", |_, _| Ok(true));
        let negated = not_fallible(g);
        assert!(
            !negated
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn not_fallible_deny() {
        let g = GuardImpl::new_fallible("no", |_, _| Ok(false));
        let negated = not_fallible(g);
        assert!(
            negated
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn not_fallible_propagates_error() {
        let g = GuardImpl::new_fallible("fail", |_, _| Err("boom".into()));
        let negated = not_fallible(g);
        assert!(
            negated
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .is_err()
        );
    }

    #[test]
    fn and_fallible_multiple_errors_first_error_wins() {
        let g1 = GuardImpl::new_fallible("a", |_, _| Err("err1".into()));
        let g2 = GuardImpl::new_fallible("b", |_, _| Err("err2".into()));
        let combined = and_fallible(vec![g1, g2]);
        let err = combined
            .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
            .unwrap_err();
        // The combinator wraps the sub-guard error; the original name is in the source chain
        assert_eq!(err.guard_name, "a AND b");
        let inner = err.source.downcast_ref::<GuardError>().unwrap();
        assert_eq!(inner.guard_name, "a");
    }

    #[test]
    fn or_fallible_all_errors_last_error_wins() {
        let g1 = GuardImpl::new_fallible("a", |_, _| Err("err1".into()));
        let g2 = GuardImpl::new_fallible("b", |_, _| Err("err2".into()));
        let combined = or_fallible(vec![g1, g2]);
        let err = combined
            .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
            .unwrap_err();
        // The combinator wraps the sub-guard error; the original name is in the source chain
        assert_eq!(err.guard_name, "a OR b");
        let inner = err.source.downcast_ref::<GuardError>().unwrap();
        assert_eq!(inner.guard_name, "b");
    }

    #[test]
    fn and_fallible_empty() {
        let combined = and_fallible(vec![]);
        assert!(
            combined
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[test]
    fn or_fallible_empty() {
        let combined = or_fallible(vec![]);
        assert!(
            !combined
                .evaluate_fallible(&serde_json::Value::Null, &serde_json::Value::Null)
                .unwrap()
        );
    }

    #[derive(Debug, PartialEq)]
    enum TestError {
        NotEligible,
        Expired,
    }

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::NotEligible => write!(f, "participant not eligible"),
                Self::Expired => write!(f, "submission deadline passed"),
            }
        }
    }

    impl std::error::Error for TestError {}

    #[test]
    fn typed_guard_preserves_the_error_type() {
        let guard: TypedGuard<u64, (), TestError> = TypedGuard::new("eligible", |deposit, _| {
            if *deposit >= 5_000 {
                Ok(true)
            } else {
                Err(TestError::NotEligible)
            }
        });
        assert!(guard.evaluate(&5_000, &()).unwrap());
        let failure = guard.evaluate(&100, &()).unwrap_err();
        assert_eq!(failure.guard, "eligible");
        assert_eq!(failure.source, TestError::NotEligible);
        assert_eq!(guard.name(), "eligible");
    }

    #[test]
    fn typed_guard_failure_displays_guard_and_source() {
        let guard: TypedGuard<u64, (), TestError> =
            TypedGuard::new("deadline", |_, _| Err(TestError::Expired));
        let failure = guard.evaluate(&1, &()).unwrap_err();
        let msg = failure.to_string();
        assert!(msg.contains("deadline"));
        assert!(msg.contains("submission deadline passed"));
    }

    #[test]
    fn typed_guard_failure_bridges_into_guard_error_downcastable() {
        let guard: TypedGuard<serde_json::Value, serde_json::Value, TestError> =
            TypedGuard::new("typed", |_, _| Err(TestError::NotEligible));
        let erased: GuardError = GuardFailure {
            guard: guard.name().to_string(),
            source: TestError::NotEligible,
        }
        .into();
        assert_eq!(erased.guard_name, "typed");
        let down = erased
            .source
            .downcast_ref::<TestError>()
            .expect("typed error preserved in the source chain");
        assert_eq!(down, &TestError::NotEligible);
    }

    #[test]
    fn typed_guard_erases_into_a_registry() {
        let guard: TypedGuard<serde_json::Value, serde_json::Value, TestError> = TypedGuard::new(
            "deposit",
            |ctx: &serde_json::Value, _: &serde_json::Value| {
                if ctx["deposit"].as_i64().unwrap_or(0) >= 5_000 {
                    Ok(true)
                } else {
                    Err(TestError::NotEligible)
                }
            },
        );
        let mut reg = GuardRegistry::new();
        reg.register(guard.erased());
        let ok = reg.evaluate_fallible(
            "deposit",
            &serde_json::json!({"deposit": 9_000}),
            &serde_json::Value::Null,
        );
        assert!(ok.unwrap());
        let err = reg
            .evaluate_fallible(
                "deposit",
                &serde_json::json!({"deposit": 1}),
                &serde_json::Value::Null,
            )
            .unwrap_err();
        assert_eq!(err.guard_name, "deposit");
        let down = err.source.downcast_ref::<TestError>().unwrap();
        assert_eq!(down, &TestError::NotEligible);
    }
}
