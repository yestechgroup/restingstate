use js_sys::{Object, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;
use xstate_wasm::{JsActorSystem, JsMachine};

fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

const HISTORY_MACHINE: &str = r#"{"id":"hist","initial":"main","context":{"fields":[]},"states":[{"name":"main","type":"compound","initial":"b1","transitions":[{"event":"SWITCH","target":"other"}],"children":[{"name":"b1","transitions":[{"event":"NEXT","target":"b2"}]},{"name":"b2"},{"name":"hist","type":"history"}]},{"name":"other","transitions":[{"event":"BACK","target":"hist"}]}]}"#;

fn persisted_of(machine: &JsMachine) -> String {
    machine.persisted_snapshot()
}

fn value_of(snapshot_json: &str) -> JsValue {
    let parsed = js_sys::JSON::parse(snapshot_json).expect("snapshot should be valid JSON");
    get(&parsed, "value")
}

fn state_value_of(machine: &JsMachine) -> JsValue {
    js_sys::JSON::parse(&machine.get_state()).expect("state should be valid JSON")
}

fn json_eq(a: &JsValue, b: &JsValue) -> bool {
    let sa = js_sys::JSON::stringify(a).unwrap().as_string().unwrap();
    let sb = js_sys::JSON::stringify(b).unwrap().as_string().unwrap();
    sa == sb
}

#[wasm_bindgen_test]
fn persisted_snapshot_includes_version_and_history() {
    let machine = JsMachine::new(HISTORY_MACHINE).expect("machine should parse");
    let persisted = js_sys::JSON::parse(&machine.persisted_snapshot())
        .expect("persisted snapshot should be valid JSON");
    assert_eq!(
        get(&persisted, "version"),
        JsValue::from_f64(1.0),
        "persisted snapshot should be versioned"
    );
    assert!(
        get(&persisted, "history").is_object(),
        "persisted snapshot should include the history map"
    );
    assert!(
        get(&persisted, "entry_times").is_object(),
        "persisted snapshot should include the entry_times map"
    );
}

#[wasm_bindgen_test]
fn js_machine_restore_round_trips_history() {
    let m1 = JsMachine::new(HISTORY_MACHINE).expect("machine should parse");
    m1.send(r#"{"type":"NEXT"}"#)
        .expect("NEXT should transition");
    m1.send(r#"{"type":"SWITCH"}"#)
        .expect("SWITCH should transition");

    let persisted = persisted_of(&m1);
    let m2 = JsMachine::restore(HISTORY_MACHINE, &persisted)
        .expect("restore should accept a persisted snapshot");

    assert_eq!(
        state_value_of(&m2),
        state_value_of(&m1),
        "restored machine should be in the same state as the persisted one"
    );
    assert_eq!(
        state_value_of(&m2),
        JsValue::from_str("other"),
        "sanity: persisted state before BACK should be other"
    );

    let s1 = m1
        .send(r#"{"type":"BACK"}"#)
        .expect("original should transition");
    let s2 = m2
        .send(r#"{"type":"BACK"}"#)
        .expect("restored should transition");
    let s1_value = value_of(&s1);
    let s2_value = value_of(&s2);
    assert!(
        json_eq(&s1_value, &s2_value),
        "original and restored machines must behave identically on the history target: {:?} vs {:?}",
        s1_value,
        s2_value
    );
    let expected = Object::new();
    Reflect::set(
        &expected,
        &JsValue::from_str("main"),
        &JsValue::from_str("b2"),
    )
    .expect("set main");
    assert!(
        json_eq(&s1_value, &JsValue::from(expected.clone())),
        "history target must return to recorded child b2, got {:?}",
        s1_value
    );
}

#[wasm_bindgen_test]
fn js_machine_restore_accepts_old_format() {
    let machine = JsMachine::restore(HISTORY_MACHINE, r#"{"value":"other","context":{}}"#)
        .expect("old-format {value, context} snapshots must remain valid");
    assert_eq!(state_value_of(&machine), JsValue::from_str("other"));
}

#[wasm_bindgen_test]
fn spawn_from_state_restores_snapshot() {
    let m1 = JsMachine::new(HISTORY_MACHINE).expect("machine should parse");
    m1.send(r#"{"type":"NEXT"}"#)
        .expect("NEXT should transition");
    m1.send(r#"{"type":"SWITCH"}"#)
        .expect("SWITCH should transition");
    let persisted = persisted_of(&m1);

    let system = JsActorSystem::new();
    system
        .spawn_from_state("restored", HISTORY_MACHINE, &persisted)
        .expect("spawn_from_state should accept a persisted snapshot");

    let snap = system.snapshot("restored").expect("actor should exist");
    let value = value_of(&snap);
    assert_eq!(
        value,
        JsValue::from_str("other"),
        "spawned actor should start in the persisted state"
    );

    system.send_to("restored", r#"{"type":"BACK"}"#);
    let snap = system.snapshot("restored").expect("actor should exist");
    let value = value_of(&snap);
    let expected = Object::new();
    Reflect::set(
        &expected,
        &JsValue::from_str("main"),
        &JsValue::from_str("b2"),
    )
    .expect("set main");
    assert!(
        json_eq(&value, &JsValue::from(expected.clone())),
        "spawned actor must return to the recorded history child b2, got {:?}",
        value
    );
}

#[wasm_bindgen_test]
fn restore_rejects_unsupported_version() {
    let result = JsMachine::restore(
        HISTORY_MACHINE,
        r#"{"value":"other","context":{},"version":99}"#,
    );
    assert!(result.is_err(), "unsupported version must be rejected");
}
