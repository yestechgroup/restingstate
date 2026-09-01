use js_sys::{Array, Function, Object, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;
use xstate_wasm::{JsMachine, clear_registry, register_action, register_guard};

fn set(obj: &JsValue, key: &str, value: JsValue) {
    let ok = Reflect::set(obj, &JsValue::from_str(key), &value)
        .unwrap_or_else(|err| panic!("Reflect::set({key}) failed: {err:?}"));
    assert!(ok, "Reflect::set({key}) rejected");
}

fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn guarded_machine(count: i64) -> String {
    format!(
        r#"{{"id":"guarded","initial":"idle","context":{{"fields":[{{"name":"count","type_name":"number","default_value":{count}}}]}},"states":[{{"name":"idle","transitions":[{{"event":"TOGGLE","target":"done","guard":{{"name":"isValid"}}}}]}},{{"name":"done"}}]}}"#
    )
}

fn action_machine() -> &'static str {
    r#"{"id":"actions","initial":"idle","context":{"fields":[]},"states":[{"name":"idle","transitions":[{"event":"INC","target":"done","actions":[{"name":"bump","args":[{"delta":5}]}]}]},{"name":"done"}]}"#
}

fn nested_machine() -> &'static str {
    r#"{"id":"nested","initial":"idle","context":{"fields":[{"name":"nested","type_name":"object","default_value":{"items":["a","b"],"meta":{"flag":true},"count":42}}]},"states":[{"name":"idle","transitions":[{"event":"CHECK","target":"done","guard":{"name":"nested"}}]},{"name":"done"}]}"#
}

fn throwing_guard_machine() -> &'static str {
    r#"{"id":"throwing","initial":"idle","context":{"fields":[]},"states":[{"name":"idle","transitions":[{"event":"GO","target":"done","guard":{"name":"boom"}}]},{"name":"done"}]}"#
}

fn state_of(machine: &JsMachine) -> JsValue {
    js_sys::JSON::parse(&machine.get_state()).expect("state should be valid JSON")
}

#[wasm_bindgen_test]
fn guard_receives_context_as_object_and_can_allow_transition() {
    clear_registry();
    let observed = Object::new();
    let observed_closure = JsValue::from(observed.clone());
    let observed_clone = observed_closure.clone();
    let guard = Closure::wrap(Box::new(move |ctx: JsValue, _event: JsValue| -> JsValue {
        let count = get(&ctx, "count").as_f64().unwrap_or(f64::NAN);
        set(&observed_closure, "count", JsValue::from_f64(count));
        set(
            &observed_closure,
            "ctxIsObject",
            JsValue::from_bool(ctx.is_object()),
        );
        JsValue::from_bool(count > 0.0)
    }) as Box<dyn FnMut(JsValue, JsValue) -> JsValue>);
    register_guard("isValid", guard.as_ref().unchecked_ref::<Function>());

    let machine = JsMachine::new(&guarded_machine(1)).expect("machine should parse");
    let snapshot = machine
        .send(r#"{"type":"TOGGLE"}"#)
        .expect("guarded transition should be allowed");

    let snap = js_sys::JSON::parse(&snapshot).expect("snapshot should be valid JSON");
    assert_eq!(
        get(&snap, "value"),
        JsValue::from_str("done"),
        "guard ctx.count > 0 should allow the transition"
    );
    assert_eq!(
        get(&observed_clone, "count"),
        JsValue::from_f64(1.0),
        "guard should receive ctx.count as a number"
    );
    assert!(
        get(&observed_clone, "ctxIsObject").is_truthy(),
        "guard should receive ctx as a JS object, not a JSON string"
    );
}

#[wasm_bindgen_test]
fn guard_returning_false_blocks_transition() {
    clear_registry();
    let observed = Object::new();
    let observed_closure = JsValue::from(observed.clone());
    let observed_clone = observed_closure.clone();
    let guard = Closure::wrap(Box::new(move |ctx: JsValue, _event: JsValue| -> JsValue {
        let calls = get(&observed_closure, "calls").as_f64().unwrap_or(0.0) + 1.0;
        set(&observed_closure, "calls", JsValue::from_f64(calls));
        set(
            &observed_closure,
            "ctxIsObject",
            JsValue::from_bool(ctx.is_object()),
        );
        let count = get(&ctx, "count").as_f64().unwrap_or(f64::NAN);
        JsValue::from_bool(count > 0.0)
    }) as Box<dyn FnMut(JsValue, JsValue) -> JsValue>);
    register_guard("isValid", guard.as_ref().unchecked_ref::<Function>());

    let machine = JsMachine::new(&guarded_machine(0)).expect("machine should parse");
    let result = machine.send(r#"{"type":"TOGGLE"}"#);

    assert!(
        result.is_none(),
        "blocked transition should not produce a snapshot"
    );
    assert_eq!(
        state_of(&machine),
        JsValue::from_str("idle"),
        "guard returning false must block the transition"
    );
    assert_eq!(
        get(&observed_clone, "calls"),
        JsValue::from_f64(1.0),
        "guard should be consulted exactly once"
    );
    assert!(
        get(&observed_clone, "ctxIsObject").is_truthy(),
        "guard should receive ctx as a JS object, not a JSON string"
    );
}

#[wasm_bindgen_test]
fn action_receives_event_and_can_mutate_context_object() {
    clear_registry();
    let store = Object::new();
    set(&store, "count", JsValue::from_f64(0.0));
    set(&store, "types", Array::new().into());
    set(&store, "argsIsArray", JsValue::FALSE);
    set(&store, "firstArgDelta", JsValue::from_f64(f64::NAN));
    let store_closure = JsValue::from(store.clone());
    let store_clone = store_closure.clone();
    let action = Closure::wrap(Box::new(move |args: JsValue, event: JsValue| {
        let event_type = get(&event, "type");
        let types = get(&store_closure, "types");
        types.unchecked_ref::<Array>().push(&event_type);

        let amount = get(&event, "amount").as_f64().unwrap_or(0.0);
        let count = get(&store_closure, "count").as_f64().unwrap_or(0.0);
        set(&store_closure, "count", JsValue::from_f64(count + amount));

        let args_is_array = Array::is_array(&args);
        set(
            &store_closure,
            "argsIsArray",
            JsValue::from_bool(args_is_array),
        );
        let delta = if args_is_array {
            get(&Array::from(&args).get(0), "delta")
                .as_f64()
                .unwrap_or(f64::NAN)
        } else {
            f64::NAN
        };
        set(&store_closure, "firstArgDelta", JsValue::from_f64(delta));
    }) as Box<dyn FnMut(JsValue, JsValue)>);
    register_action("bump", action.as_ref().unchecked_ref::<Function>());

    let machine = JsMachine::new(action_machine()).expect("machine should parse");
    let snapshot = machine
        .send(r#"{"type":"INC","amount":5}"#)
        .expect("action transition should happen");

    let snap = js_sys::JSON::parse(&snapshot).expect("snapshot should be valid JSON");
    assert_eq!(
        get(&snap, "value"),
        JsValue::from_str("done"),
        "transition with action should reach the target state"
    );
    let types = get(&store_clone, "types");
    assert_eq!(
        Array::from(&types).get(0),
        JsValue::from_str("INC"),
        "action should receive event.type as a real string"
    );
    assert_eq!(
        get(&store_clone, "count"),
        JsValue::from_f64(5.0),
        "action should read event.amount and mutate its context object"
    );
    assert!(
        get(&store_clone, "argsIsArray").is_truthy(),
        "action should receive args as a JS array, not a JSON string"
    );
    assert_eq!(
        get(&store_clone, "firstArgDelta"),
        JsValue::from_f64(5.0),
        "action args elements should be real JS objects"
    );
}

#[wasm_bindgen_test]
fn nested_context_values_arrive_as_js_objects_and_arrays() {
    clear_registry();
    let observed = Object::new();
    let observed_closure = JsValue::from(observed.clone());
    let observed_clone = observed_closure.clone();
    let guard = Closure::wrap(Box::new(move |ctx: JsValue, _event: JsValue| -> JsValue {
        let nested = get(&ctx, "nested");
        set(
            &observed_closure,
            "ctxIsObject",
            JsValue::from_bool(ctx.is_object()),
        );
        set(
            &observed_closure,
            "nestedIsObject",
            JsValue::from_bool(nested.is_object()),
        );
        let items = get(&nested, "items");
        let items_is_array = Array::is_array(&items);
        set(
            &observed_closure,
            "itemsIsArray",
            JsValue::from_bool(items_is_array),
        );
        let items_length = if items_is_array {
            Array::from(&items).length() as f64
        } else {
            f64::NAN
        };
        set(
            &observed_closure,
            "itemsLength",
            JsValue::from_f64(items_length),
        );
        let flag = get(&get(&nested, "meta"), "flag")
            .as_bool()
            .unwrap_or(false);
        set(&observed_closure, "metaFlag", JsValue::from_bool(flag));
        let count = get(&nested, "count").as_f64().unwrap_or(f64::NAN);
        set(&observed_closure, "count", JsValue::from_f64(count));
        JsValue::TRUE
    }) as Box<dyn FnMut(JsValue, JsValue) -> JsValue>);
    register_guard("nested", guard.as_ref().unchecked_ref::<Function>());

    let machine = JsMachine::new(nested_machine()).expect("machine should parse");
    machine
        .send(r#"{"type":"CHECK"}"#)
        .expect("nested guard transition should be allowed");

    assert!(
        get(&observed_clone, "ctxIsObject").is_truthy(),
        "ctx should be a JS object"
    );
    assert!(
        get(&observed_clone, "nestedIsObject").is_truthy(),
        "ctx.nested should be a JS object, not a JSON string"
    );
    assert!(
        get(&observed_clone, "itemsIsArray").is_truthy(),
        "ctx.nested.items should be a JS array"
    );
    assert_eq!(
        get(&observed_clone, "itemsLength"),
        JsValue::from_f64(2.0),
        "ctx.nested.items should have length 2"
    );
    assert!(
        get(&observed_clone, "metaFlag").is_truthy(),
        "ctx.nested.meta.flag should be a real boolean true"
    );
    assert_eq!(
        get(&observed_clone, "count"),
        JsValue::from_f64(42.0),
        "ctx.nested.count should be a real number"
    );
}

#[wasm_bindgen_test]
fn guard_exception_is_logged_and_defaults_to_allow() {
    clear_registry();
    let console = get(&js_sys::global(), "console");
    let original_error = get(&console, "error");
    let errors = Array::new();
    let errors_clone = errors.clone();
    let capture = Closure::wrap(Box::new(move |message: JsValue| {
        errors_clone.push(&message);
    }) as Box<dyn FnMut(JsValue)>);
    set(
        &console,
        "error",
        capture.as_ref().unchecked_ref::<Function>().into(),
    );

    let throwing: Function =
        Function::new_with_args("ctx, event", "throw new Error('boom guard failure');");
    register_guard("boom", &throwing);

    let machine = JsMachine::new(throwing_guard_machine()).expect("machine should parse");
    let result = machine.send(r#"{"type":"GO"}"#);

    assert!(
        result.is_some(),
        "guard exception should default to allowing the transition"
    );
    let snap = js_sys::JSON::parse(&result.expect("snapshot checked above"))
        .expect("snapshot should be valid JSON");
    assert_eq!(get(&snap, "value"), JsValue::from_str("done"));

    assert_eq!(
        errors.length(),
        1,
        "guard exception should be logged via console.error"
    );
    let message = errors.get(0).as_string().unwrap_or_default();
    assert!(
        message.contains("boom"),
        "console.error should name the failing guard, got: {message:?}"
    );

    set(&console, "error", original_error);
}
