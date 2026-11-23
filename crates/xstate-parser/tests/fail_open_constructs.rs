//! Issue #46: the parser must fail closed on behavior-bearing constructs it
//! cannot represent in IR — inline guards/actions, identifier references,
//! higher-order guard calls, guard params, and present-but-non-literal scalars.
//!
//! Before the fix every case below parsed green while silently producing a
//! *different* machine (guard dropped → transition always taken, action
//! dropped, id replaced by `machine-N`).

use serde_json::json;
use xstate_parser::{parse, parse_all};

/// Wrap a state-body snippet into a full machine source, mirroring the
/// authoring shapes used in the issue reproduction.
fn wrap(body: &str) -> String {
    format!(
        "const logIt = () => console.log('x');\n\
         const m = createMachine({{\n\
           id: 'm', initial: 'a', context: {{ count: 0 }},\n\
           states: {{\n\
             a: {{ {body} }},\n\
             b: {{ type: 'final' }}\n\
           }}\n\
         }});\n"
    )
}

// ---------------------------------------------------------------------------
// Behavior-bearing inline constructs must be rejected, never dropped
// ---------------------------------------------------------------------------

#[test]
fn behavior_bearing_constructs_are_rejected_not_dropped() {
    let cases = [
        // inline guard, array-form transition
        r#"on: { GO: [{ target: 'b', guard: ({ context }) => context.count > 0 }] }"#,
        // inline guard, single-object transition
        r#"on: { GO: { target: 'b', guard: (ctx) => ctx.count > 0 } }"#,
        // identifier action reference
        r#"on: { GO: { target: 'b', actions: [logIt] } }"#,
        // inline transition action
        r#"on: { GO: { target: 'b', actions: () => console.log('x') } }"#,
        // inline entry action
        r#"entry: [() => console.log('x')], on: { GO: 'b' }"#,
        // inline exit action
        r#"exit: [function () {}], on: { GO: 'b' }"#,
        // higher-level guard (docs cheatsheet) — rejected until IR represents compositions
        r#"on: { GO: { target: 'b', guard: and(['x']) } }"#,
        // higher-level guards, or()/not() forms
        r#"on: { GO: { target: 'b', guard: or(['x', 'y']) } }"#,
        r#"on: { GO: { target: 'b', guard: not('x') } }"#,
    ];
    for body in cases {
        let src = wrap(body);
        let err = parse(&src)
            .expect_err("behavior-bearing construct was silently dropped instead of rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("Unsupported"),
            "rejection must be a typed parse error, got: {msg}"
        );
    }
}

#[test]
fn parse_all_rejects_behavior_bearing_constructs_too() {
    // The extract-bin path (parse_all) previously swallowed these errors via
    // its per-machine leniency — it must propagate the same rejections.
    let cases = [
        r#"on: { GO: [{ target: 'b', guard: ({ context }) => context.count > 0 }] }"#,
        r#"entry: [() => console.log('x')], on: { GO: 'b' }"#,
        r#"on: { GO: { target: 'b', guard: and(['x']) } }"#,
        r#"on: { GO: { target: 'b', actions: [logIt] } }"#,
    ];
    for body in cases {
        let src = wrap(body);
        parse_all(&src).expect_err("parse_all silently dropped a behavior-bearing construct");
    }
}

// ---------------------------------------------------------------------------
// Guard params: carried into IR, never silently stripped
// ---------------------------------------------------------------------------

#[test]
fn guard_params_are_carried_into_ir_never_stripped() {
    let src = wrap(
        r#"on: { GO: { target: 'b', guard: { type: 'isValid', params: { maxLength: 50 } } } }"#,
    );
    let machine =
        parse(&src).expect("guard params are serializable data and must be carried into IR");
    let a = machine.states.iter().find(|s| s.name == "a").unwrap();
    let guard = a.transitions[0].guard.as_ref().expect("guard must survive");
    assert_eq!(guard.name, "isValid");
    assert_eq!(guard.params, Some(json!({ "maxLength": 50 })));
}

#[test]
fn guard_object_without_params_round_trips() {
    let src = wrap(r#"on: { GO: { target: 'b', guard: { type: 'isValid' } } }"#);
    let machine = parse(&src).expect("string-typed object guards are the accepted subset");
    let guard = machine.states[0].transitions[0].guard.as_ref().unwrap();
    assert_eq!(guard.name, "isValid");
    assert_eq!(guard.params, None);
}

#[test]
fn guard_params_with_non_literal_values_are_rejected() {
    // params are data; a function or identifier inside them cannot be carried
    let src =
        wrap(r#"on: { GO: { target: 'b', guard: { type: 'isValid', params: { fn: () => 1 } } } }"#);
    parse(&src).expect_err("non-literal guard params must be rejected, not silently nulled");
}

// ---------------------------------------------------------------------------
// Scalars: present-but-unresolvable must error, never fall back silently
// ---------------------------------------------------------------------------

#[test]
fn present_but_unresolvable_id_is_rejected_not_replaced() {
    let src = "const m = createMachine({ id: 'm' as const, initial: 'a', \
               states: { a: { on: { GO: 'b' } }, b: { type: 'final' } } });";
    let err = parse(src).expect_err(
        "an id that is present but not a plain literal must error, not fall back to 'machine-0'",
    );
    assert!(
        err.to_string().contains("id"),
        "error must name the offending construct: {err}"
    );
}

#[test]
fn parse_all_does_not_replace_present_id_with_machine_n() {
    // The silent "machine-0" replacement lived on the parse_all/extract-bin path.
    let src = "const m = createMachine({ id: 'scalar-probe' as const, initial: 'a', \
               states: { a: { on: { GO: 'b' } }, b: { type: 'final' } } });";
    let err = parse_all(src)
        .expect_err("parse_all silently replaced a present-but-unresolvable id with 'machine-0'");
    assert!(
        err.to_string().contains("id"),
        "error must name the construct: {err}"
    );
}

#[test]
fn present_but_unresolvable_target_is_rejected() {
    // A target that silently resolves to None changes the transition into an
    // (internal) transition — a different machine.
    let src = "const TARGET = 'b'; const m = createMachine({ id: 'm', initial: 'a', \
               states: { a: { on: { GO: { target: TARGET } } }, b: { type: 'final' } } });";
    parse(&src).expect_err("present-but-unresolvable transition target must be rejected");
}

#[test]
fn unknown_state_type_string_is_rejected() {
    // `type: 'compond'` currently degrades to atomic — a different machine.
    let src = "const m = createMachine({ id: 'm', initial: 'a', \
               states: { a: { type: 'compond', initial: 'a1', states: { a1: { type: 'final' } } } } });";
    parse(src).expect_err("unknown state type strings must be rejected, not coerced to atomic");
}

// ---------------------------------------------------------------------------
// Accepted subset keeps working (guard against over-rejection)
// ---------------------------------------------------------------------------

#[test]
fn accepted_declarative_subset_still_parses() {
    // string guard, string actions, string arrays, assign calls, string entry/exit
    let src = wrap(
        r#"entry: ['focus'], exit: ['blur'],
           on: {
             GO: { target: 'b', guard: 'isReady', actions: ['log', 'track'] },
             TICK: { target: 'a', actions: assign({ count: ({ context }) => context.count + 1 }) },
             BACK: [{ target: 'b', guard: { type: 'gate', params: { level: 2 } } }, 'b']
           }"#,
    );
    let machine = parse(&src).expect("the declarative subset must remain accepted");
    let a = machine.states.iter().find(|s| s.name == "a").unwrap();
    assert_eq!(a.entry, vec!["focus".to_string()]);
    assert_eq!(a.exit, vec!["blur".to_string()]);
    let go = a.transitions.iter().find(|t| t.event == "GO").unwrap();
    assert_eq!(go.guard.as_ref().unwrap().name, "isReady");
    assert_eq!(go.actions.len(), 2);
    let tick = a.transitions.iter().find(|t| t.event == "TICK").unwrap();
    assert_eq!(tick.actions[0].name, "assign");
    let back = a.transitions.iter().find(|t| t.event == "BACK").unwrap();
    assert_eq!(back.guard.as_ref().unwrap().name, "gate");
    assert_eq!(
        back.guard.as_ref().unwrap().params,
        Some(json!({ "level": 2 }))
    );
}

#[test]
fn missing_id_still_synthesizes_on_parse_all() {
    // Anonymous machines remain the lenient contract of parse_all — the
    // fallback is allowed only when the id prop is *missing*, not unresolvable.
    let src = "const m = createMachine({ initial: 'a', \
               states: { a: { on: { GO: 'b' } }, b: { type: 'final' } } });";
    let machines = parse_all(src).expect("anonymous machines stay accepted on parse_all");
    assert_eq!(machines[0].id, "machine-0");
}

#[test]
fn plain_template_literal_scalars_are_accepted() {
    let src = "const m = createMachine({ id: `m`, initial: 'a', \
               states: { a: { on: { GO: 'b' } }, b: { type: 'final' } } });";
    let machine = parse(src).expect("plain (non-interpolated) template literals are literals");
    assert_eq!(machine.id, "m");
}
