// Native actor-system abuse robustness matrix — issue #29.
use xstate_backend_native::NativeActorSystem;
use xstate_core::backend::{ActorHandle, ActorSystem};
use xstate_ir::{Action, ContextSchema, InvokeConfig, Machine, StateNode, StateType, Transition};

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

fn machine(id: &str, initial: &str, states: Vec<StateNode>) -> Machine {
    Machine {
        id: id.into(),
        initial: initial.into(),
        context: ContextSchema { fields: vec![] },
        states,
        setup: None,
    }
}

fn toggle(id: &str) -> Machine {
    machine(
        id,
        "inactive",
        vec![
            atomic("inactive", vec![trans("toggle", "active")]),
            atomic("active", vec![trans("toggle", "inactive")]),
        ],
    )
}

fn toggle_value(snapshot: &serde_json::Value) -> String {
    snapshot["value"].as_str().unwrap_or_default().to_string()
}

#[test]
fn send_to_unknown_id_is_a_noop() {
    let system = NativeActorSystem::new();
    system.send_to("ghost", serde_json::json!({"type": "toggle"}));
    assert_eq!(system.len(), 0);
    assert!(system.snapshot("ghost").is_none());
    assert!(system.status("ghost").is_none());
}

#[test]
fn duplicate_spawn_id_replaces_without_panic() {
    let system = NativeActorSystem::new();
    let first = system.spawn_machine("a", toggle("first"));
    first.send(serde_json::json!({"type": "toggle"}));
    assert_eq!(toggle_value(&system.snapshot("a").unwrap()), "active");

    let _second = system.spawn_machine("a", toggle("second"));
    assert_eq!(
        system.len(),
        1,
        "duplicate id must not create a second actor"
    );
    assert_eq!(
        toggle_value(&system.snapshot("a").unwrap()),
        "inactive",
        "the replacement machine's initial snapshot wins"
    );
}

#[test]
fn double_stop_is_idempotent() {
    let system = NativeActorSystem::new();
    let handle = system.spawn_machine("a", toggle("t"));
    handle.send(serde_json::json!({"type": "toggle"}));

    handle.stop();
    assert_eq!(
        system.status("a"),
        Some(xstate_core::backend::ActorStatus::Stopped)
    );
    handle.stop();
    assert_eq!(
        system.status("a"),
        Some(xstate_core::backend::ActorStatus::Stopped)
    );

    handle.send(serde_json::json!({"type": "toggle"}));
    assert_eq!(
        toggle_value(&system.snapshot("a").unwrap()),
        "active",
        "a stopped actor must ignore events"
    );
}

#[test]
fn stop_parent_mid_invoke_cleans_up_child() {
    let mut loading = atomic(
        "loading",
        vec![trans("done.invoke.runner.child", "completed")],
    );
    loading.invoke = Some(InvokeConfig {
        src: "child".into(),
        id: None,
        on_done: vec![],
        on_error: vec![],
    });
    let parent = machine(
        "runner",
        "idle",
        vec![
            atomic("idle", vec![trans("LOAD", "loading")]),
            loading,
            atomic("completed", vec![]),
        ],
    );
    let child = machine(
        "child",
        "working",
        vec![atomic("working", vec![trans("FINISH", "end")]), {
            let mut end = atomic("end", vec![]);
            end.node_type = StateType::Final;
            end
        }],
    );

    let system = NativeActorSystem::new();
    system.register_machine("child", child);
    let parent = system.spawn_machine("runner", parent);

    parent.send(serde_json::json!({"type": "LOAD"}));
    assert_eq!(
        system.status("runner.child"),
        Some(xstate_core::backend::ActorStatus::Running),
        "control: the child must be running mid-invoke"
    );

    parent.stop();
    assert_eq!(
        system.status("runner"),
        Some(xstate_core::backend::ActorStatus::Stopped)
    );
    assert_eq!(
        system.status("runner.child"),
        Some(xstate_core::backend::ActorStatus::Stopped),
        "stopping the parent mid-invoke must cascade to the child"
    );

    system.send_to("runner.child", serde_json::json!({"type": "FINISH"}));
    assert_eq!(
        system.status("runner.child"),
        Some(xstate_core::backend::ActorStatus::Stopped),
        "stopped child must ignore events"
    );
}

#[test]
fn invoke_src_without_registered_machine_is_ignored_not_panic() {
    let mut loading = atomic(
        "loading",
        vec![trans("done.invoke.runner.child", "completed")],
    );
    loading.invoke = Some(InvokeConfig {
        src: "unregistered".into(),
        id: None,
        on_done: vec![],
        on_error: vec![],
    });
    let parent = machine(
        "runner",
        "idle",
        vec![
            atomic("idle", vec![trans("LOAD", "loading")]),
            loading,
            atomic("completed", vec![]),
        ],
    );

    let system = NativeActorSystem::new();
    let parent = system.spawn_machine("runner", parent);
    parent.send(serde_json::json!({"type": "LOAD"}));

    assert!(system.status("runner.unregistered").is_none());
    assert_eq!(system.len(), 1);
    assert_eq!(
        toggle_value(&system.snapshot("runner").unwrap()),
        "loading",
        "parent keeps running when the invoke src is unknown"
    );
}

#[test]
fn machine_with_unknown_initial_state_spawns_without_panicking() {
    let mut broken = toggle("t");
    broken.initial = "ghost".into();

    let system = NativeActorSystem::new();
    let handle = system.spawn_machine("broken", broken);

    assert_eq!(system.len(), 1);
    assert!(system.snapshot("broken").is_some());
    handle.send(serde_json::json!({"type": "toggle"}));
    assert_eq!(
        system.status("broken"),
        Some(xstate_core::backend::ActorStatus::Running),
        "the actor must stay alive and ignore events it cannot resolve (#29)"
    );
}

#[test]
fn machine_with_unknown_compound_initial_child_spawns_without_panicking() {
    let mut broken = atomic("broken", vec![trans("GO", "idle")]);
    broken.node_type = StateType::Compound;
    broken.initial = Some("ghost".into());
    broken.children = vec![atomic("real", vec![])];

    let system = NativeActorSystem::new();
    let handle = system.spawn_machine("broken", machine("m", "broken", vec![broken]));

    assert_eq!(system.len(), 1);
    assert!(system.snapshot("broken").is_some());
    handle.send(serde_json::json!({"type": "GO"}));
    assert_eq!(
        system.status("broken"),
        Some(xstate_core::backend::ActorStatus::Running)
    );
}

#[test]
fn malformed_events_through_the_system_do_not_panic() {
    let system = NativeActorSystem::new();
    let handle = system.spawn_machine("a", toggle("t"));

    for event in [
        serde_json::Value::Null,
        serde_json::json!({"type": 123}),
        serde_json::json!([]),
        serde_json::json!({}),
        serde_json::json!({"type": "toggle", "payload": "x".repeat(64 * 1024)}),
    ] {
        handle.send(event);
    }
    assert_eq!(toggle_value(&system.snapshot("a").unwrap()), "active");
}

#[test]
fn persisted_spawn_with_garbage_snapshot_is_a_typed_error() {
    let system = NativeActorSystem::new();

    let wrong_version = serde_json::json!({"value": "inactive", "context": {}, "version": 99});
    assert!(
        system
            .spawn_machine_from_persisted("a", toggle("t"), &wrong_version)
            .is_err()
    );

    let unknown_state = serde_json::json!({"value": "limbo", "context": {}, "version": 1});
    assert!(
        system
            .spawn_machine_from_persisted("b", toggle("t"), &unknown_state)
            .is_err()
    );

    assert_eq!(
        system.len(),
        0,
        "failed restores must not leave actors behind"
    );
}

#[test]
fn self_sending_machine_is_bounded_not_wedged() {
    // A degenerate machine that re-sends its own event forever used to wedge
    // the run loop; each dispatch must now be bounded (#29).
    let looper = machine(
        "looper",
        "idle",
        vec![atomic(
            "idle",
            vec![Transition {
                event: "LOOP".into(),
                target: None,
                guard: None,
                actions: vec![
                    Action {
                        name: "assign".into(),
                        args: vec![serde_json::json!({
                            "$assign_ops": [
                                { "op": "add", "field": "n", "value": 1 }
                            ]
                        })],
                    },
                    Action {
                        name: "xstate.send_to".into(),
                        args: vec![
                            serde_json::json!("looper"),
                            serde_json::json!({"type": "LOOP"}),
                        ],
                    },
                ],
                description: None,
                delay_ms: None,
                targets: vec![],
            }],
        )],
    );
    let mut looper = looper;
    looper.context = ContextSchema {
        fields: vec![xstate_ir::ContextField {
            name: "n".into(),
            type_name: "number".into(),
            default_value: serde_json::json!(0),
        }],
    };

    let system = NativeActorSystem::new();
    let handle = system.spawn_machine("looper", looper);
    handle.send(serde_json::json!({"type": "LOOP"}));

    let rounds = system.snapshot("looper").unwrap()["context"]["n"]
        .as_i64()
        .expect("counter must stay numeric");
    assert!(rounds > 0, "the loop must actually run");
    assert!(
        rounds <= 10_001,
        "dispatch must be bounded, ran {rounds} rounds"
    );
    assert_eq!(
        system.status("looper"),
        Some(xstate_core::backend::ActorStatus::Running),
        "the actor survives a bounded dispatch"
    );
}
