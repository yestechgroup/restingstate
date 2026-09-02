// After-transition parity — issue #34.
//
// Delayed (`after`) transitions must fire only through the deadline check
// (`entry + delay_ms` against the injected Clock); they must never fire
// eventlessly.
use xstate_core::StateValue;
use xstate_core::clock::MockClock;
use xstate_core::interpreter::Interpreter;
use xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

fn no_guards() -> impl Fn(&str, &serde_json::Value, &serde_json::Value) -> bool {
    |_: &str, _: &serde_json::Value, _: &serde_json::Value| true
}

fn no_actions() -> impl FnMut(&str, &[serde_json::Value], &serde_json::Value) {
    |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {}
}

fn atomic(name: &str, transitions: Vec<Transition>) -> StateNode {
    StateNode {
        name: name.into(),
        description: None,
        initial: None,
        entry: vec![],
        exit: vec![],
        transitions,
        children: vec![],
        node_type: StateType::Atomic,
        tags: vec![],
        invoke: None,
        on_done: vec![],
        output: None,
    }
}

fn delayed(event: &str, target: &str, delay_ms: u64) -> Transition {
    Transition {
        event: event.into(),
        target: Some(target.into()),
        guard: None,
        actions: vec![],
        description: None,
        delay_ms: Some(delay_ms),
        targets: vec![],
    }
}

fn trans(event: &str, target: &str) -> Transition {
    Transition {
        event: event.into(),
        target: Some(target.into()),
        guard: None,
        actions: vec![],
        description: None,
        delay_ms: None,
        targets: vec![],
    }
}

fn machine(initial: &str, states: Vec<StateNode>) -> Machine {
    Machine {
        id: "m".into(),
        initial: initial.into(),
        context: ContextSchema { fields: vec![] },
        states,
        setup: None,
    }
}

fn send_at(
    interpreter: &Interpreter,
    state: &StateValue,
    event: &serde_json::Value,
    now: u64,
) -> Option<StateValue> {
    let mut context = interpreter.initial_context();
    interpreter.send(
        state,
        event,
        &mut context,
        &no_guards(),
        &mut no_actions(),
        &MockClock::new(now),
    )
}

#[test]
fn unrelated_event_before_deadline_does_not_fire_after() {
    // green --after(1000)--> yellow; an unrelated event that is handled
    // (self-transition) must not drag the after-transition along at t=0.
    // Regression: the post-event resolve_always pass matched the delayed
    // transition's `event: ""` and fired it with no deadline check (#34).
    let m = machine(
        "green",
        vec![
            atomic(
                "green",
                vec![trans("STAY", "green"), delayed("", "yellow", 1000)],
            ),
            atomic("yellow", vec![]),
        ],
    );
    let interpreter = Interpreter::new(m);
    let state = interpreter.initial_state();

    let next = send_at(
        &interpreter,
        &state,
        &serde_json::json!({"type": "STAY"}),
        0,
    );
    assert_eq!(
        next,
        Some(StateValue::Atomic("green".into())),
        "a handled event before the deadline must not fire the after-transition"
    );
}

#[test]
fn self_transition_re_entry_resets_the_deadline() {
    // green --after(100)--> gone; green --STAY--> green (external self).
    // STAY at t=50 re-enters green and must re-arm the deadline:
    //   t=101: unreset deadline (100) would have fired -> must NOT fire
    //   t=150: reset deadline (50+100) -> must fire
    let m = machine(
        "green",
        vec![
            atomic(
                "green",
                vec![trans("STAY", "green"), delayed("", "gone", 100)],
            ),
            atomic("gone", vec![]),
        ],
    );
    let interpreter = Interpreter::new(m);
    let mut state = interpreter.initial_state();

    state = send_at(
        &interpreter,
        &state,
        &serde_json::json!({"type": "STAY"}),
        50,
    )
    .unwrap();
    assert_eq!(state, StateValue::Atomic("green".into()));

    let next = send_at(
        &interpreter,
        &state,
        &serde_json::json!({"type": "NOOP"}),
        101,
    );
    assert_eq!(
        next, None,
        "self-transition at t=50 must have re-armed the t=100 deadline"
    );
    assert!(
        send_at(
            &interpreter,
            &state,
            &serde_json::json!({"type": "NOOP"}),
            150
        )
        .is_some()
    );
}

#[test]
fn first_send_after_deadline_fires_the_after_transition() {
    // Poll model: the after-transition fires on the first send whose Clock
    // reading is >= entry + delay; not before.
    let m = machine(
        "green",
        vec![
            atomic("green", vec![delayed("", "yellow", 100)]),
            atomic("yellow", vec![]),
        ],
    );
    let interpreter = Interpreter::new(m);
    let state = interpreter.initial_state();

    assert_eq!(
        send_at(
            &interpreter,
            &state,
            &serde_json::json!({"type": "NOOP"}),
            10
        ),
        None,
        "one tick before the deadline must not fire"
    );
    assert_eq!(
        send_at(
            &interpreter,
            &state,
            &serde_json::json!({"type": "NOOP"}),
            100
        ),
        Some(StateValue::Atomic("yellow".into())),
        "first send at the deadline must fire"
    );
}

#[test]
fn exiting_the_source_cancels_the_after_transition_permanently() {
    // green --after(100)--> gone; green --GO--> red (terminal).
    // After leaving green, its pending after must never fire.
    let m = machine(
        "green",
        vec![
            atomic("green", vec![trans("GO", "red"), delayed("", "gone", 100)]),
            atomic("red", vec![]),
            atomic("gone", vec![]),
        ],
    );
    let interpreter = Interpreter::new(m);
    let state = interpreter.initial_state();

    let red = send_at(&interpreter, &state, &serde_json::json!({"type": "GO"}), 50).unwrap();
    assert_eq!(red, StateValue::Atomic("red".into()));

    assert_eq!(
        send_at(
            &interpreter,
            &red,
            &serde_json::json!({"type": "NOOP"}),
            1000
        ),
        None,
        "the exited state's after-transition must be cancelled permanently"
    );
    assert_eq!(
        send_at(
            &interpreter,
            &red,
            &serde_json::json!({"type": "NOOP"}),
            5000
        ),
        None
    );
}

#[test]
fn sibling_always_and_after_coexist() {
    // green carries BOTH a delayed transition (listed first) and a real
    // always transition, gated by the "opened" guard so green can actually
    // hold while the deadline runs. Pre-#34 the eventless resolve_always
    // pass matched the delayed transition (event "") and fired the after
    // instantly, consuming the always' chance.
    //
    // opened=true:  STAY -> the always fires immediately (delayed skipped)
    // opened=false: the always stays blocked and the after fires at its
    //               own deadline.
    use xstate_ir::Guard;
    let always = Transition {
        event: String::new(),
        target: Some("wait".into()),
        guard: Some(Guard {
            name: "opened".into(),
        }),
        actions: vec![],
        description: None,
        delay_ms: None,
        targets: vec![],
    };
    let m = machine(
        "green",
        vec![
            atomic(
                "green",
                vec![delayed("", "done", 100), always, trans("STAY", "green")],
            ),
            atomic("wait", vec![]),
            atomic("done", vec![]),
        ],
    );
    let interpreter = Interpreter::new(m);
    let state = interpreter.initial_state();
    let guards = |name: &str, ctx: &serde_json::Value, _: &serde_json::Value| {
        name != "opened" || ctx["opened"] == serde_json::json!(true)
    };
    let send_at_guarded = |ctx: &mut serde_json::Value,
                           event: &serde_json::Value,
                           opened: bool,
                           now: u64|
     -> Option<StateValue> {
        ctx["opened"] = serde_json::json!(opened);
        interpreter.send(
            &state,
            event,
            ctx,
            &guards,
            &mut no_actions(),
            &MockClock::new(now),
        )
    };

    let mut ctx = interpreter.initial_context();
    let always_fired =
        send_at_guarded(&mut ctx, &serde_json::json!({"type": "STAY"}), true, 0).unwrap();
    assert_eq!(
        always_fired,
        StateValue::Atomic("wait".into()),
        "the sibling always must fire immediately; the after must wait for its deadline"
    );

    let mut ctx = interpreter.initial_context();
    let held = send_at_guarded(&mut ctx, &serde_json::json!({"type": "STAY"}), false, 0).unwrap();
    assert_eq!(
        held,
        StateValue::Atomic("green".into()),
        "with the always guard false, the STAY self-transition must not drag the after along"
    );

    assert_eq!(
        send_at_guarded(&mut ctx, &serde_json::json!({"type": "NOOP"}), false, 99),
        None,
        "the always' guard must not block, nor the after fire early"
    );
    assert_eq!(
        send_at_guarded(&mut ctx, &serde_json::json!({"type": "NOOP"}), false, 100),
        Some(StateValue::Atomic("done".into())),
        "the always-loop must not consume the after's chance: it fires at its deadline"
    );
}

#[test]
fn deepest_due_after_deadline_wins_first() {
    // p (compound, initial c) has after(10) -> z and its child c has
    // after(10) -> z2. Both deadlines are due at t=10; resolve_after walks
    // the active path deepest-first (path.iter().rev()), so the leaf c fires
    // and its target z2 (outside p) wins — p's after is cancelled by p's
    // exit and z must never be reached. (#34: deepest-first when multiple
    // after-deadlines are due in one resolve.)
    let c = atomic("c", vec![delayed("", "z2", 10)]);
    let z2 = atomic("z2", vec![]);
    let mut p = atomic("p", vec![delayed("", "z", 10)]);
    p.node_type = StateType::Compound;
    p.initial = Some("c".into());
    p.children = vec![c, atomic("d", vec![])];
    let m = machine("p", vec![p, z2, atomic("z", vec![])]);
    let interpreter = Interpreter::new(m);
    let state = interpreter.initial_state();

    assert_eq!(
        send_at(
            &interpreter,
            &state,
            &serde_json::json!({"type": "NOOP"}),
            10
        ),
        Some(StateValue::Atomic("z2".into())),
        "the leaf's due after-deadline must fire before its ancestor's"
    );
}

#[test]
fn guard_on_after_transition_is_evaluated_at_fire_time() {
    use xstate_ir::Guard;
    // The guard is checked when the deadline is due, not on entry: a false
    // guard leaves the machine in place and the transition is retried on
    // later sends (once the guard passes).
    let ready = Transition {
        event: String::new(),
        target: Some("done".into()),
        guard: Some(Guard {
            name: "ready".into(),
        }),
        actions: vec![],
        description: None,
        delay_ms: Some(50),
        targets: vec![],
    };
    let m = machine("s", vec![atomic("s", vec![ready]), atomic("done", vec![])]);
    let interpreter = Interpreter::new(m);
    let state = interpreter.initial_state();

    let guards = |name: &str, ctx: &serde_json::Value, _: &serde_json::Value| {
        name != "ready" || ctx["ready"] == serde_json::json!(true)
    };

    let mut context = interpreter.initial_context();
    let not_due = interpreter.send(
        &state,
        &serde_json::json!({"type": "NOOP"}),
        &mut context,
        &guards,
        &mut no_actions(),
        &MockClock::new(10),
    );
    assert_eq!(not_due, None, "before the deadline nothing fires");

    let blocked = interpreter.send(
        &state,
        &serde_json::json!({"type": "NOOP"}),
        &mut context,
        &guards,
        &mut no_actions(),
        &MockClock::new(100),
    );
    assert_eq!(
        blocked, None,
        "a false guard at fire time must leave the machine unchanged"
    );

    context["ready"] = serde_json::json!(true);
    let fired = interpreter.send(
        &state,
        &serde_json::json!({"type": "NOOP"}),
        &mut context,
        &guards,
        &mut no_actions(),
        &MockClock::new(100),
    );
    assert_eq!(
        fired,
        Some(StateValue::Atomic("done".into())),
        "the after must be retried on a later send once the guard passes"
    );
}

#[test]
fn named_event_with_delay_fires_only_via_deadline() {
    // Decision pinned here: a transition carrying delay_ms NEVER matches in
    // send_along_path — not even when its (named) event arrives directly.
    // resolve_after is the sole firer; sending the named event early is
    // simply too soon.
    let named_delay = Transition {
        event: "AFTER_100".into(),
        target: Some("done".into()),
        guard: None,
        actions: vec![],
        description: None,
        delay_ms: Some(100),
        targets: vec![],
    };
    let m = machine(
        "s",
        vec![atomic("s", vec![named_delay]), atomic("done", vec![])],
    );
    let interpreter = Interpreter::new(m);
    let state = interpreter.initial_state();

    assert_eq!(
        send_at(
            &interpreter,
            &state,
            &serde_json::json!({"type": "AFTER_100"}),
            10
        ),
        None,
        "sending the named event before the deadline must not fire it"
    );
    assert_eq!(
        send_at(
            &interpreter,
            &state,
            &serde_json::json!({"type": "AFTER_100"}),
            100
        ),
        Some(StateValue::Atomic("done".into())),
        "at the deadline resolve_after fires it (named or not)"
    );
}

#[test]
fn after_targeting_self_re_arms_its_deadline() {
    // s --after(100)--> s: firing re-enters s, so the deadline re-arms —
    // otherwise every later send would re-fire it.
    let m = machine(
        "s",
        vec![
            atomic("s", vec![delayed("", "s", 100)]),
            atomic("done", vec![]),
        ],
    );
    let interpreter = Interpreter::new(m);
    let state = interpreter.initial_state();

    let fired = send_at(
        &interpreter,
        &state,
        &serde_json::json!({"type": "NOOP"}),
        150,
    )
    .unwrap();
    assert_eq!(fired, StateValue::Atomic("s".into()));

    assert_eq!(
        send_at(
            &interpreter,
            &fired,
            &serde_json::json!({"type": "NOOP"}),
            160
        ),
        None,
        "the self-fire must have re-armed the deadline to 150+100"
    );
    assert!(
        send_at(
            &interpreter,
            &fired,
            &serde_json::json!({"type": "NOOP"}),
            250
        )
        .is_some()
    );
}
