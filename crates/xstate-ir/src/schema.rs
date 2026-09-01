//! JSON Schema generation for the machine IR — issue #30, feature `schema`.
//!
//! The generated document is committed at `schema/machine-ir.schema.json`;
//! `crates/xstate-ir/tests/schema_golden.rs` enforces that it stays in sync.

use crate::Machine;
use schemars::Schema;

pub fn machine_ir_schema() -> Schema {
    schemars::schema_for!(Machine)
}

pub fn machine_ir_schema_json() -> String {
    let mut json = serde_json::to_string_pretty(&machine_ir_schema()).expect("schema serializes");
    json.push('\n');
    json
}
