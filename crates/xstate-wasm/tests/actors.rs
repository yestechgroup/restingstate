use js_sys::{Function, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;
use xstate_wasm::{JsActorSystem, clear_registry, register_guard};

fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn value_of(snapshot_json: &str) -> JsValue {
    let parsed = js_sys::JSON::parse(snapshot_json).expect("snapshot should be valid JSON");
    get(&parsed, "value")
}

const PARENT_MACHINE: &str = r#"{"id":"runner","initial":"idle","context":{"fields":[]},"states":[{"name":"idle","transitions":[{"event":"LOAD","target":"loading"}]},{"name":"loading","invoke":{"src":"child"},"transitions":[{"event":"done.invoke.runner.child","target":"completed"}]},{"name":"completed"}]}"#;

const CHILD_MACHINE: &str = r#"{"id":"child","initial":"working","context":{"fields":[]},"states":[{"name":"working","transitions":[{"event":"FINISH","target":"end"}]},{"name":"end","type":"final"}]}"#;

fn guarded_machine(count: i64) -> String {
    format!(
        r#"{{"id":"guarded","initial":"off","context":{{"fields":[{{"name":"count","type_name":"number","default_value":{count}}}]}}, "states":[{{"name":"off","transitions":[{{"event":"TOGGLE","target":"on","guard":{{"name":"isOk"}}}}]}},{{"name":"on"}}]}}"#
    )
}

#[wasm_bindgen_test]
fn invoke_parent_child_end_to_end_under_js_actor_system() {
    clear_registry();
    let system = JsActorSystem::new();
    system
        .register_machine("child", CHILD_MACHINE)
        .expect("child machine should parse");
    system
        .spawn("runner", PARENT_MACHINE)
        .expect("parent machine should parse");

    system.send_to("runner", r#"{"type":"LOAD"}"#);
    assert_eq!(
        value_of(&system.snapshot("runner").expect("parent snapshot")),
        JsValue::from_str("loading"),
        "parent should be in the invoking state"
    );
    assert_eq!(
        system.status("runner.child"),
        Some("running".to_string()),
        "child actor should be auto-spawned on entry to the invoke state"
    );

    system.send_to("runner.child", r#"{"type":"FINISH"}"#);
    assert_eq!(
        system.status("runner.child"),
        Some("done".to_string()),
        "child reaching a final state should report done"
    );
    assert_eq!(
        value_of(&system.snapshot("runner").expect("parent snapshot")),
        JsValue::from_str("completed"),
        "parent should transition on done.invoke.runner.child"
    );
}

#[wasm_bindgen_test]
fn spawned_actor_evaluates_registered_guard() {
    clear_registry();
    let guard = Closure::wrap(Box::new(move |ctx: JsValue, _event: JsValue| -> JsValue {
        let count = get(&ctx, "count").as_f64().unwrap_or(f64::NAN);
        JsValue::from_bool(count > 0.0)
    }) as Box<dyn FnMut(JsValue, JsValue) -> JsValue>);
    register_guard("isOk", guard.as_ref().unchecked_ref::<Function>());

    let system = JsActorSystem::new();

    system
        .spawn("m0", &guarded_machine(0))
        .expect("machine with count 0 should parse");
    system.send_to("m0", r#"{"type":"TOGGLE"}"#);
    assert_eq!(
        value_of(&system.snapshot("m0").expect("m0 snapshot")),
        JsValue::from_str("off"),
        "guard returning false must block the transition in a spawned actor"
    );

    system
        .spawn("m5", &guarded_machine(5))
        .expect("machine with count 5 should parse");
    system.send_to("m5", r#"{"type":"TOGGLE"}"#);
    assert_eq!(
        value_of(&system.snapshot("m5").expect("m5 snapshot")),
        JsValue::from_str("on"),
        "guard returning true should allow the transition in a spawned actor"
    );
}

#[wasm_bindgen_test]
fn existing_spawn_semantics_unchanged() {
    clear_registry();
    let system = JsActorSystem::new();
    system
        .spawn_reducer("counter", r#"{"count":0}"#)
        .expect("reducer spawn should work");
    system.send_to("counter", r#"{"type":"INC"}"#);
    let snap =
        js_sys::JSON::parse(&system.snapshot("counter").expect("snapshot")).expect("valid JSON");
    assert_eq!(get(&snap, "count"), JsValue::from_f64(0.0));
    assert_eq!(system.len(), 1);
    assert!(!system.is_empty());
}
