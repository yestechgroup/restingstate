//! `validate_machine_json` + extended structural validation — issue #30.
use xstate_core::{ValidateError, validate_machine_json, validate_machine_structure};
use xstate_ir::Machine;

const TOGGLE: &str = r#"{
  "id": "toggle",
  "initial": "inactive",
  "context": { "fields": [] },
  "states": [
    { "name": "inactive", "transitions": [{ "event": "toggle", "target": "active" }] },
    { "name": "active", "transitions": [{ "event": "toggle", "target": "inactive" }] }
  ]
}"#;

fn valid_machine() -> Machine {
    serde_json::from_str(TOGGLE).unwrap()
}

#[test]
fn valid_machine_json_is_ok() {
    assert_eq!(validate_machine_json(TOGGLE), Ok(()));
}

#[test]
fn parse_error_is_typed_and_helpful() {
    for garbage in ["", "not json", "[]", "{}", r#"{"id": "m"}"#] {
        match validate_machine_json(garbage) {
            Err(err @ ValidateError::Parse(_)) => {
                assert!(
                    err.to_string().contains("not valid JSON"),
                    "parse error should mention JSON validity: {err}"
                );
            }
            other => panic!("expected Parse error for {garbage:?}, got {other:?}"),
        }
    }
}

#[test]
fn unknown_initial_state_is_rejected() {
    let json = r#"{
        "id": "m", "initial": "ghost", "context": { "fields": [] },
        "states": [{ "name": "a" }]
    }"#;
    match validate_machine_json(json) {
        Err(ValidateError::Invalid(e)) => {
            assert!(
                e.to_string().contains("initial state 'ghost' not found"),
                "unexpected error: {e}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn unknown_compound_initial_child_is_rejected() {
    let json = r#"{
        "id": "m", "initial": "parent", "context": { "fields": [] },
        "states": [{
            "name": "parent", "type": "compound", "initial": "ghost",
            "children": [{ "name": "real" }]
        }]
    }"#;
    assert!(matches!(
        validate_machine_json(json),
        Err(ValidateError::Invalid(_))
    ));
}

#[test]
fn unknown_transition_target_is_rejected() {
    let json = r#"{
        "id": "m", "initial": "a", "context": { "fields": [] },
        "states": [
            { "name": "a", "transitions": [{ "event": "GO", "target": "ghost" }] },
            { "name": "b" }
        ]
    }"#;
    match validate_machine_json(json) {
        Err(ValidateError::Invalid(e)) => {
            let msg = e.to_string();
            assert!(
                msg.contains("'a'") && msg.contains("unknown state 'ghost'"),
                "error should name the source state and missing target: {msg}"
            );
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn unknown_multi_target_entry_is_rejected() {
    let json = r#"{
        "id": "m", "initial": "a", "context": { "fields": [] },
        "states": [
            { "name": "a", "transitions": [{ "event": "GO", "targets": ["b", "ghost"] }] },
            { "name": "b" }
        ]
    }"#;
    assert!(matches!(
        validate_machine_json(json),
        Err(ValidateError::Invalid(_))
    ));
}

#[test]
fn nested_state_targets_are_validated_recursively() {
    let json = r#"{
        "id": "m", "initial": "outer", "context": { "fields": [] },
        "states": [{
            "name": "outer", "type": "compound", "initial": "inner",
            "children": [
                { "name": "inner", "transitions": [{ "event": "GO", "target": "ghost" }] }
            ]
        }]
    }"#;
    assert!(matches!(
        validate_machine_json(json),
        Err(ValidateError::Invalid(_))
    ));
}

#[test]
fn cross_compound_and_dotted_targets_resolve() {
    // Bare name found anywhere in the tree + root-anchored dotted paths (#30).
    let json = r#"{
        "id": "m", "initial": "main", "context": { "fields": [] },
        "states": [
            {
                "name": "main", "type": "compound", "initial": "idle",
                "transitions": [{ "event": "SWITCH", "target": "other" }],
                "children": [
                    { "name": "idle", "transitions": [{ "event": "NEXT", "target": "active" }] },
                    { "name": "active" },
                    { "name": "hist", "type": "history" }
                ]
            },
            { "name": "other", "transitions": [{ "event": "BACK", "target": "main.hist" }] }
        ]
    }"#;
    assert_eq!(validate_machine_json(json), Ok(()));
}

#[test]
fn structure_validation_matches_json_entry_point() {
    let machine: Machine = serde_json::from_str(TOGGLE).unwrap();
    assert_eq!(validate_machine_structure(&machine), Ok(()));

    let mut broken = machine.clone();
    broken.initial = "ghost".into();
    assert!(validate_machine_structure(&broken).is_err());
}

#[test]
fn interpreter_validate_machine_shares_the_same_checks() {
    // Delegation contract: Interpreter::validate_machine and the free fn must agree.
    let mut broken = valid_machine();
    broken.states[0].transitions[0].target = Some("ghost".into());
    assert!(xstate_core::Interpreter::validate_machine(&broken).is_err());
    assert!(validate_machine_structure(&broken).is_err());
}
