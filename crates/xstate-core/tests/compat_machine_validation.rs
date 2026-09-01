//! Compat-machine validation sweep — issue #30.
//!
//! Every hand-crafted compatibility vector's `machine_ir` must parse into
//! `xstate_ir::Machine` and pass structural validation.
use std::path::{Path, PathBuf};
use xstate_core::validate_machine_json;

fn collect_json_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("compat machines dir exists") {
        let path = entry.expect("readable dir entry").path();
        if path.is_dir() {
            collect_json_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            out.push(path);
        }
    }
}

#[test]
fn every_compat_machine_ir_parses_and_validates() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/compatibility/machines");
    let mut files = Vec::new();
    collect_json_files(&dir, &mut files);
    files.sort();
    assert!(
        files.len() >= 25,
        "expected the full compat vector set, found {} files",
        files.len()
    );

    let mut failures = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("readable vector");
        let wrapper: serde_json::Value = serde_json::from_str(&text).expect("vector is valid JSON");
        let Some(machine_ir) = wrapper.get("machine_ir") else {
            failures.push(format!("{}: no `machine_ir` field", file.display()));
            continue;
        };
        let ir_text = serde_json::to_string(machine_ir).expect("IR serializes");
        if let Err(err) = validate_machine_json(&ir_text) {
            failures.push(format!("{}: {err}", file.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "compat machines rejected by validate_machine_json (do not weaken validation — \
         investigate and justify instead):\n{}",
        failures.join("\n")
    );
}
