//! Pins the actor-flow conventions documented in docs/contracts.md.

use std::sync::{Arc, Mutex};

use xstate_backend_native::NativeActorSystem;
use xstate_core::ActionImpl;
use xstate_core::actions::ActionRegistry;
use xstate_core::actor_logic::PromiseLogic;
use xstate_core::backend::{ActorHandle, ActorStatus, ActorSystem};
use xstate_core::guards::GuardRegistry;
use xstate_ir::{
    Action, ContextField, ContextSchema, InvokeConfig, Machine, StateNode, StateType, Transition,
};

fn field(name: &str, default_value: serde_json::Value) -> ContextField {
    ContextField {
        name: name.into(),
        type_name: "json".into(),
        default_value,
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

fn action_trans(event: &str, actions: Vec<Action>) -> Transition {
    Transition {
        event: event.into(),
        target: None,
        guard: None,
        actions,
        description: None,
        delay_ms: None,
        targets: vec![],
    }
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

fn final_node(name: &str) -> StateNode {
    StateNode {
        name: name.into(),
        description: None,
        initial: None,
        entry: vec![],
        exit: vec![],
        transitions: vec![],
        children: vec![],
        node_type: StateType::Final,
        tags: vec![],
        invoke: None,
        on_done: vec![],
        output: None,
    }
}

fn machine(id: &str, initial: &str, fields: Vec<ContextField>, states: Vec<StateNode>) -> Machine {
    Machine {
        id: id.into(),
        initial: initial.into(),
        context: ContextSchema { fields },
        states,
        setup: None,
    }
}

fn system_capturing_events(
    action_name: &'static str,
) -> (NativeActorSystem, Arc<Mutex<Vec<serde_json::Value>>>) {
    let captured: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let system = NativeActorSystem::new();
    system.set_registry_factory(move || {
        let sink = sink.clone();
        let mut actions = ActionRegistry::new();
        actions.register(ActionImpl::new(action_name, move |_args, event, _ctx| {
            sink.lock().unwrap().push(event.clone());
        }));
        (GuardRegistry::new(), actions)
    });
    (system, captured)
}

#[test]
fn done_invoke_event_type_and_output_payload_are_pinned() {
    let mut loading = atomic(
        "loading",
        vec![{
            let mut t = trans("done.invoke.worker", "finished");
            t.actions = vec![Action {
                name: "captureDone".into(),
                args: vec![],
            }];
            t
        }],
    );
    loading.invoke = Some(InvokeConfig {
        src: "worker".into(),
        id: Some("worker".into()),
        on_done: vec![],
        on_error: vec![],
    });

    let parent = machine(
        "runner",
        "loading",
        vec![],
        vec![loading, atomic("finished", vec![])],
    );
    let child = machine(
        "worker",
        "working",
        vec![field("result", serde_json::json!(42))],
        vec![
            atomic("working", vec![trans("FINISH", "end")]),
            final_node("end"),
        ],
    );

    let (system, captured) = system_capturing_events("captureDone");
    system.spawn_machine("runner", parent);
    let child = system.spawn_machine_with_parent("worker", child, "runner");

    child.send(serde_json::json!({"type": "FINISH"}));

    assert_eq!(
        system.status("worker"),
        Some(ActorStatus::Done),
        "child reaching a final state must report Done"
    );
    assert_eq!(
        system.snapshot("runner").unwrap()["value"],
        "finished",
        "parent must transition on the exact done.invoke.<id> event name"
    );

    let events = captured.lock().unwrap();
    assert_eq!(events.len(), 1, "exactly one done event must be delivered");
    assert_eq!(events[0]["type"], "done.invoke.worker");
    assert_eq!(events[0]["output"], serde_json::json!({"result": 42}));
}

#[test]
fn invoke_without_explicit_id_delivers_done_invoke_parent_dot_src() {
    let mut loading = atomic(
        "loading",
        vec![trans("done.invoke.runner.helper", "finished")],
    );
    loading.invoke = Some(InvokeConfig {
        src: "helper".into(),
        id: None,
        on_done: vec![],
        on_error: vec![],
    });

    let parent = machine(
        "runner",
        "idle",
        vec![],
        vec![
            atomic("idle", vec![trans("START", "loading")]),
            loading,
            atomic("finished", vec![]),
        ],
    );
    let helper = machine(
        "helper",
        "working",
        vec![],
        vec![
            atomic("working", vec![trans("FINISH", "end")]),
            final_node("end"),
        ],
    );

    let system = NativeActorSystem::new();
    system.register_machine("helper", helper);
    let parent = system.spawn_machine("runner", parent);

    parent.send(serde_json::json!({"type": "START"}));
    assert_eq!(
        system.status("runner.helper"),
        Some(ActorStatus::Running),
        "invoke without an id must auto-spawn the child as parent.src"
    );

    system.send_to("runner.helper", serde_json::json!({"type": "FINISH"}));
    assert_eq!(
        system.snapshot("runner").unwrap()["value"],
        "finished",
        "parent must receive done.invoke.parent.src"
    );
}

#[test]
fn error_platform_event_type_and_error_payload_are_pinned() {
    let idle = {
        let mut t = trans("error.platform.pchild", "failed");
        t.actions = vec![Action {
            name: "captureError".into(),
            args: vec![],
        }];
        atomic("idle", vec![t])
    };

    let parent = machine(
        "watcher",
        "idle",
        vec![],
        vec![idle, atomic("failed", vec![])],
    );

    let (system, captured) = system_capturing_events("captureError");
    system.spawn_machine("watcher", parent);
    let child = system.spawn_logic_with_parent(
        "pchild",
        Box::new(PromiseLogic::new("pchild", async {
            Err(serde_json::json!({"message": "boom"}))
        })),
        "watcher",
    );

    child.send(serde_json::json!({"type": "POLL"}));

    assert_eq!(
        system.status("pchild"),
        Some(ActorStatus::Error),
        "a rejecting promise child must report Error"
    );
    assert_eq!(
        system.snapshot("watcher").unwrap()["value"],
        "failed",
        "parent must transition on the exact error.platform.<id> event name"
    );

    let events = captured.lock().unwrap();
    assert_eq!(events.len(), 1, "exactly one error event must be delivered");
    assert_eq!(events[0]["type"], "error.platform.pchild");
    assert_eq!(events[0]["error"], serde_json::json!({"message": "boom"}));
}

#[test]
fn xstate_send_to_arg_positions_are_pinned() {
    let parent = machine(
        "p",
        "idle",
        vec![],
        vec![atomic(
            "idle",
            vec![action_trans(
                "TRIGGER",
                vec![Action {
                    name: "xstate.send_to".into(),
                    args: vec![
                        serde_json::json!("c"),
                        serde_json::json!({"type": "POKE", "n": 1}),
                    ],
                }],
            )],
        )],
    );
    let child = machine(
        "c",
        "idle",
        vec![],
        vec![
            atomic("idle", vec![trans("POKE", "poked")]),
            atomic("poked", vec![]),
        ],
    );

    let system = NativeActorSystem::new();
    system.spawn_machine("c", child);
    let parent = system.spawn_machine("p", parent);

    parent.send(serde_json::json!({"type": "TRIGGER"}));

    assert_eq!(
        system.snapshot("c").unwrap()["value"],
        "poked",
        "args[0] must be the target actor id"
    );
    assert_eq!(
        system.snapshot("c").unwrap()["context"],
        serde_json::json!({}),
        "args[1] is delivered as the event, not merged into context"
    );
}

#[test]
fn xstate_send_parent_arg_position_is_pinned() {
    let parent = machine(
        "p",
        "idle",
        vec![],
        vec![
            atomic("idle", vec![trans("CHILD_DONE", "done")]),
            atomic("done", vec![]),
        ],
    );
    let child = machine(
        "c",
        "idle",
        vec![],
        vec![atomic(
            "idle",
            vec![action_trans(
                "PING",
                vec![Action {
                    name: "xstate.send_parent".into(),
                    args: vec![serde_json::json!({"type": "CHILD_DONE", "blob": 7})],
                }],
            )],
        )],
    );

    let system = NativeActorSystem::new();
    system.spawn_machine("p", parent);
    let child = system.spawn_machine_with_parent("c", child, "p");

    child.send(serde_json::json!({"type": "PING"}));

    assert_eq!(
        system.snapshot("p").unwrap()["value"],
        "done",
        "args[0] must be delivered to the parent as the event"
    );
}

#[test]
fn xstate_spawn_arg_positions_and_default_child_id_are_pinned() {
    let leaf = machine("leaf", "on", vec![], vec![atomic("on", vec![])]);
    let explicit = machine(
        "spawnExplicit",
        "idle",
        vec![],
        vec![atomic(
            "idle",
            vec![action_trans(
                "SPAWN",
                vec![Action {
                    name: "xstate.spawn".into(),
                    args: vec![
                        serde_json::json!("leaf"),
                        serde_json::json!("spawnExplicit.kid"),
                    ],
                }],
            )],
        )],
    );
    let default_id = machine(
        "spawnDefault",
        "idle",
        vec![],
        vec![atomic(
            "idle",
            vec![action_trans(
                "SPAWN",
                vec![Action {
                    name: "xstate.spawn".into(),
                    args: vec![serde_json::json!("leaf")],
                }],
            )],
        )],
    );

    let system = NativeActorSystem::new();
    system.register_machine("leaf", leaf);
    let explicit = system.spawn_machine("spawnExplicit", explicit);
    let default_id = system.spawn_machine("spawnDefault", default_id);

    explicit.send(serde_json::json!({"type": "SPAWN"}));
    default_id.send(serde_json::json!({"type": "SPAWN"}));

    assert_eq!(
        system.status("spawnExplicit.kid"),
        Some(ActorStatus::Running),
        "args[0] is the registered machine name, args[1] the explicit child id"
    );
    assert_eq!(
        system.status("spawnDefault.child"),
        Some(ActorStatus::Running),
        "a missing args[1] must default the child id to spawner.child"
    );
    assert_eq!(
        system.children_of("spawnExplicit"),
        vec!["spawnExplicit.kid"]
    );
    assert_eq!(
        system.children_of("spawnDefault"),
        vec!["spawnDefault.child"]
    );
}
