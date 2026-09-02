// WASM boundary robustness matrix — issue #29.
use wasm_bindgen_test::*;
use xstate_wasm::{JsActorSystem, JsMachine};

const TOGGLE_MACHINE: &str = r#"{
  "id": "toggle",
  "initial": "inactive",
  "context": { "fields": [] },
  "states": [
    { "name": "inactive", "transitions": [{ "event": "toggle", "target": "active", "actions": [], "targets": [] }] },
    { "name": "active", "transitions": [{ "event": "toggle", "target": "inactive", "actions": [], "targets": [] }] }
  ]
}"#;

const BROKEN_INITIAL_MACHINE: &str = r#"{
  "id": "m",
  "initial": "ghost",
  "context": { "fields": [] },
  "states": [
    { "name": "a", "transitions": [], "children": [], "entry": [], "exit": [], "tags": [], "on_done": [] }
  ]
}"#;

const BROKEN_COMPOUND_MACHINE: &str = r#"{
  "id": "m",
  "initial": "broken",
  "context": { "fields": [] },
  "states": [
    {
      "name": "broken",
      "type": "compound",
      "initial": "ghost",
      "children": [
        { "name": "real", "transitions": [], "children": [], "entry": [], "exit": [], "tags": [], "on_done": [] }
      ],
      "transitions": [], "entry": [], "exit": [], "tags": [], "on_done": []
    }
  ]
}"#;

fn garbage_inputs() -> Vec<&'static str> {
    vec![
        "",
        "not json",
        "[]",
        "{}",
        r#"{"foo": 1}"#,
        r#"123"#,
        r#"null"#,
    ]
}

#[wasm_bindgen_test]
fn js_machine_new_rejects_garbage_with_err() {
    for garbage in garbage_inputs() {
        assert!(
            JsMachine::new(garbage).is_err(),
            "JsMachine::new({garbage:?}) must return Err, not panic"
        );
    }
}

#[wasm_bindgen_test]
fn js_machine_new_rejects_semantically_broken_machines() {
    assert!(
        JsMachine::new(BROKEN_INITIAL_MACHINE).is_err(),
        "unknown initial state must be a typed Err at the boundary (#29)"
    );
    assert!(
        JsMachine::new(BROKEN_COMPOUND_MACHINE).is_err(),
        "unknown compound initial child must be a typed Err at the boundary (#29)"
    );
}

#[wasm_bindgen_test]
fn js_machine_new_accepts_valid_machine() {
    let machine = JsMachine::new(TOGGLE_MACHINE).expect("toggle machine is valid IR");
    let snap = machine.snapshot();
    assert!(snap.contains("inactive"));
}

#[wasm_bindgen_test]
fn js_machine_send_with_invalid_json_returns_none() {
    let machine = JsMachine::new(TOGGLE_MACHINE).expect("valid machine");
    assert!(machine.send("not json").is_none());
    assert!(machine.send("[]").is_none());
    assert!(machine.send("").is_none());
    // Still functional afterwards (#29).
    assert!(machine.send(r#"{"type":"toggle"}"#).is_some());
}

#[wasm_bindgen_test]
fn js_machine_send_with_deeply_nested_event_is_ignored() {
    let machine = JsMachine::new(TOGGLE_MACHINE).expect("valid machine");
    let depth = 5_000;
    let mut event = String::from(r#"{"type":"toggle"}"#);
    for _ in 0..depth {
        event = format!(r#"{{"wrap":{event}}}"#);
    }
    // serde_json's recursion limit turns this into a typed parse error -> None.
    let _ = machine.send(&event);
    assert!(machine.send(r#"{"type":"toggle"}"#).is_some());
}

#[wasm_bindgen_test]
fn js_actor_system_spawn_rejects_garbage_with_err() {
    let system = JsActorSystem::new();
    for garbage in garbage_inputs() {
        assert!(
            system.spawn("a", garbage).is_err(),
            "spawn({garbage:?}) must return Err, not panic"
        );
        assert!(
            system.register_machine("m", garbage).is_err(),
            "register_machine({garbage:?}) must return Err, not panic"
        );
    }
    assert_eq!(system.len(), 0, "no actor may be left behind by failures");
}

#[wasm_bindgen_test]
fn js_actor_system_spawn_rejects_semantically_broken_machines() {
    let system = JsActorSystem::new();
    assert!(system.spawn("a", BROKEN_INITIAL_MACHINE).is_err());
    assert!(system.spawn("b", BROKEN_COMPOUND_MACHINE).is_err());
    assert_eq!(system.len(), 0);
}

#[wasm_bindgen_test]
fn send_to_with_invalid_json_is_silently_ignored() {
    let system = JsActorSystem::new();
    system.spawn("m", TOGGLE_MACHINE).expect("valid machine");

    system.send_to("m", "not json");
    system.send_to("m", "{");
    system.send_to("m", "");

    let snap = system.snapshot("m").expect("actor must still exist");
    assert!(snap.contains("inactive"), "snapshot must be unchanged");

    system.send_to("m", r#"{"type":"toggle"}"#);
    let snap = system.snapshot("m").expect("actor must still exist");
    assert!(snap.contains("active"), "system must stay functional (#29)");
}

#[wasm_bindgen_test]
fn send_to_unknown_id_is_a_noop() {
    let system = JsActorSystem::new();
    system.spawn("m", TOGGLE_MACHINE).expect("valid machine");
    system.send_to("ghost", r#"{"type":"toggle"}"#);
    assert_eq!(system.len(), 1);
    assert!(system.snapshot("ghost").is_none());
    assert!(system.status("ghost").is_none());
}

#[wasm_bindgen_test]
fn restore_with_wrong_version_or_malformed_snapshot_is_err() {
    let good = r#"{"value":"inactive","context":{},"version":1}"#;
    assert!(
        JsMachine::restore(TOGGLE_MACHINE, good).is_ok(),
        "control: a well-formed snapshot restores"
    );

    let wrong_version = r#"{"value":"inactive","context":{},"version":99}"#;
    assert!(JsMachine::restore(TOGGLE_MACHINE, wrong_version).is_err());

    let negative_version = r#"{"value":"inactive","context":{},"version":-3}"#;
    assert!(JsMachine::restore(TOGGLE_MACHINE, negative_version).is_err());

    let missing_value = r#"{"context":{},"version":1}"#;
    assert!(JsMachine::restore(TOGGLE_MACHINE, missing_value).is_err());

    let unknown_state = r#"{"value":"limbo","context":{},"version":1}"#;
    assert!(JsMachine::restore(TOGGLE_MACHINE, unknown_state).is_err());

    assert!(JsMachine::restore(TOGGLE_MACHINE, "not json").is_err());
    assert!(JsMachine::restore(TOGGLE_MACHINE, "").is_err());
    assert!(JsMachine::restore("not json", good).is_err());
    assert!(JsMachine::restore("", good).is_err());
}

#[wasm_bindgen_test]
fn spawn_from_state_with_wrong_version_or_malformed_snapshot_is_err() {
    let system = JsActorSystem::new();

    let good = r#"{"value":"inactive","context":{},"version":1}"#;
    system
        .spawn_from_state("ok", TOGGLE_MACHINE, good)
        .expect("control: well-formed snapshot spawns");
    assert_eq!(system.status("ok"), Some("running".to_string()));

    let wrong_version = r#"{"value":"inactive","context":{},"version":99}"#;
    assert!(
        system
            .spawn_from_state("a", TOGGLE_MACHINE, wrong_version)
            .is_err()
    );

    let unknown_state = r#"{"value":"limbo","context":{},"version":1}"#;
    assert!(
        system
            .spawn_from_state("b", TOGGLE_MACHINE, unknown_state)
            .is_err()
    );

    assert!(
        system
            .spawn_from_state("c", TOGGLE_MACHINE, "not json")
            .is_err()
    );
    assert!(system.spawn_from_state("d", "not json", good).is_err());
    assert!(
        system
            .spawn_from_state("e", BROKEN_INITIAL_MACHINE, good)
            .is_err()
    );
}

#[wasm_bindgen_test]
fn js_machine_new_with_deeply_nested_machine_json_is_err() {
    let depth = 10_000;
    let mut nested = String::from("0");
    for _ in 0..depth {
        nested = format!("[{nested}]");
    }
    let machine_json = String::from(
        r#"{"id":"m","initial":"a","context":{"fields":[{"name":"x","type_name":"number","default_value":"#,
    ) + &nested
        + r#"}]}],"states":[{"name":"a"}]}"#;
    assert!(
        JsMachine::new(&machine_json).is_err(),
        "10k-deep context defaults must be a typed Err, not a crash (#29)"
    );
}
