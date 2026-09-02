//! Golden schema + regen harness — issue #30.
//!
//! Regenerate after an intentional IR schema change with:
//! `XSTATE_REGEN_SCHEMA=1 cargo test -p xstate-ir --features schema`
//! then commit the updated `schema/machine-ir.schema.json` with the change.
#![cfg(feature = "schema")]

use std::path::{Path, PathBuf};
use xstate_ir::machine_ir_schema_json;

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/machine-ir.schema.json")
}

#[test]
fn committed_schema_matches_generated() {
    let generated = machine_ir_schema_json();
    let path = golden_path();
    if std::env::var_os("XSTATE_REGEN_SCHEMA").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).expect("schema dir creatable");
        std::fs::write(&path, &generated).expect("golden writable");
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "missing golden {path:?} ({err}); regenerate with \
             XSTATE_REGEN_SCHEMA=1 cargo test -p xstate-ir --features schema"
        )
    });
    assert_eq!(
        generated, committed,
        "schema drift: `schema/machine-ir.schema.json` no longer matches the IR types. \
         Regenerate with XSTATE_REGEN_SCHEMA=1 cargo test -p xstate-ir --features schema \
         and commit the result."
    );
}

#[test]
fn schema_describes_core_ir_types() {
    let schema = machine_ir_schema_json();
    for needle in [
        "Machine",
        "StateNode",
        "StateType",
        "Transition",
        "ContextSchema",
        "SetupConfig",
        "InvokeConfig",
        "ActorDef",
        "atomic",
        "compound",
        "parallel",
        "final",
        "history",
    ] {
        assert!(
            schema.contains(needle),
            "generated schema should mention {needle:?}"
        );
    }
}

#[test]
fn generated_schema_is_valid_json_object() {
    let parsed: serde_json::Value =
        serde_json::from_str(&machine_ir_schema_json()).expect("schema is valid JSON");
    assert!(parsed.is_object(), "root schema must be a JSON object");
}
