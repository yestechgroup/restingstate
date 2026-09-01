//! `validateMachine` boundary export — issue #30.
use wasm_bindgen_test::*;
use xstate_wasm::validate_machine;

const TOGGLE_MACHINE: &str = r#"{
  "id": "toggle",
  "initial": "inactive",
  "context": { "fields": [] },
  "states": [
    { "name": "inactive", "transitions": [{ "event": "toggle", "target": "active" }] },
    { "name": "active", "transitions": [{ "event": "toggle", "target": "inactive" }] }
  ]
}"#;

#[wasm_bindgen_test]
fn valid_ir_is_ok() {
    assert_eq!(validate_machine(TOGGLE_MACHINE), Ok("ok".to_string()));
}

#[wasm_bindgen_test]
fn bad_initial_is_err_with_helpful_message() {
    let broken = r#"{
        "id": "m", "initial": "ghost", "context": { "fields": [] },
        "states": [{ "name": "a" }]
    }"#;
    let err = validate_machine(broken).expect_err("unknown initial must be Err");
    assert!(
        err.as_string()
            .unwrap()
            .contains("initial state 'ghost' not found"),
        "unexpected message: {err:?}"
    );
}

#[wasm_bindgen_test]
fn unknown_target_is_err_with_helpful_message() {
    let broken = r#"{
        "id": "m", "initial": "a", "context": { "fields": [] },
        "states": [
            { "name": "a", "transitions": [{ "event": "GO", "target": "ghost" }] },
            { "name": "b" }
        ]
    }"#;
    let err = validate_machine(broken).expect_err("unknown target must be Err");
    assert!(
        err.as_string().unwrap().contains("unknown state 'ghost'"),
        "unexpected message: {err:?}"
    );
}

#[wasm_bindgen_test]
fn malformed_json_is_err_not_panic() {
    for garbage in ["", "not json", "[]", "{}"] {
        assert!(
            validate_machine(garbage).is_err(),
            "validate_machine({garbage:?}) must return Err"
        );
    }
}
