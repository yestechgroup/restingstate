//! # restingstate
//!
//! XState-compatible statechart compiler and runtime for Rust.
//!
//! This crate is the umbrella dependency: it re-exports the public crates of
//! the workspace so downstream projects (e.g. EnvHarness) depend on one name.
//! See `docs/contracts.md` for the dependency contract and
//! `docs/runtime-paths.md` for which runtime path supports which features.
//!
//! ## Re-exports
//!
//! | Crate | Role |
//! |-------|------|
//! | [`xstate_ir`] | Language-neutral machine IR (serde types, validation, JSON schema) |
//! | [`xstate_core`] | Interpreter, snapshots, guards/actions registries, actor logic, backend traits |
//! | [`xstate_parser`] | TypeScript machine configs → IR (OXC-based) |
//! | [`xstate_backend_native`] | Synchronous, WASM-compatible actor backend |
//! | [`xstate_actor`] | Tokio-based async actor backend |
//! | [`xstate_codegen`] | IR → Rust code generation (flat FSMs) |
//! | [`xstate_macro`] | `#[xstate("path.ts")]` proc macro (flat FSMs) |
//!
//! ## Quick start
//!
//! ```no_run
//! use restingstate::prelude::*;
//!
//! let machine = parse(
//!     r#"createMachine({ id: 't', initial: 'a',
//!         states: { a: { on: { E: 'b' } }, b: {} } })"#,
//! )
//! .unwrap();
//! let interp = Interpreter::new(machine);
//! let snapshot = interp.snapshot(&interp.initial_state(), &serde_json::json!({}));
//! # let _ = snapshot;
//! ```

pub use xstate_actor;
pub use xstate_backend_native;
pub use xstate_codegen;
pub use xstate_core;
pub use xstate_ir;
pub use xstate_macro;
pub use xstate_parser;

/// The one-stop import for the common runtime surface.
pub mod prelude {
    pub use xstate_core::{
        ActionRegistry, ActorLogic, Clock, GuardRegistry, InstantClock, Interpreter, MachineLogic,
        MockClock, Snapshot, StateValue, validate_machine_json,
    };
    pub use xstate_ir::Machine;
    pub use xstate_parser::parse;
}

#[cfg(test)]
mod tests {
    #[test]
    fn umbrella_end_to_end() {
        use crate::prelude::*;
        let machine = parse(
            r#"createMachine({ id: 't', initial: 'a', states: { a: { on: { E: 'b' } }, b: {} } })"#,
        )
        .unwrap();
        let interp = Interpreter::new(machine);
        let state = interp.initial_state();
        let mut context = serde_json::json!({});
        let next = interp
            .send(
                &state,
                &serde_json::json!({"type": "E"}),
                &mut context,
                &|_, _, _| true,
                &mut |_, _, _| {},
                &MockClock::default(),
            )
            .expect("transition fires");
        assert_eq!(next, StateValue::Atomic("b".into()));
    }

    #[test]
    fn validate_machine_json_is_reachable() {
        let json = r#"{"id":"t","initial":"a","context":{"fields":[]},"states":[{"name":"a","type":"atomic","transitions":[{"event":"E","target":"b"}]},{"name":"b","type":"atomic","transitions":[]}]}"#;
        assert!(crate::prelude::validate_machine_json(json).is_ok());
    }
}
