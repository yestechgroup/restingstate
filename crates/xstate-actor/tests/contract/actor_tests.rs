use xstate_actor::ActorSystem;
use xstate_core::Interpreter;
use xstate_core::actor_logic::{ActorLogic, ReducerLogic};
use xstate_core::clock::MockClock;
use xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

fn toggle_machine() -> Machine {
    Machine {
        id: "toggle".into(),
        initial: "inactive".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "inactive".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "TOGGLE".into(),
                    target: Some("active".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
            },
            StateNode {
                name: "active".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "TOGGLE".into(),
                    target: Some("inactive".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
            },
        ],
        setup: None,
    }
}

#[tokio::test]
async fn spawn_and_send_reducer() {
    let system = ActorSystem::new();
    let logic = Box::new(ReducerLogic::new(
        "counter",
        serde_json::json!({"count": 0}),
        |state, event| {
            let mut s = state.clone();
            match event["type"].as_str() {
                Some("INC") => {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(c + 1));
                }
                Some("DEC") => {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(c - 1));
                }
                Some("RESET") => {
                    s.as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(0));
                }
                _ => {}
            }
            (s, vec![])
        },
    ));

    let id = system.spawn_logic(logic, "counter".into());
    assert_eq!(id, "counter");

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["count"], 0);

    system.send_to(&id, serde_json::json!({"type": "INC"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["count"], 1);

    system.send_to(&id, serde_json::json!({"type": "INC"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["count"], 2);

    system.send_to(&id, serde_json::json!({"type": "DEC"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["count"], 1);

    system.send_to(&id, serde_json::json!({"type": "RESET"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["count"], 0);
}

#[tokio::test]
async fn stop_actor() {
    let system = ActorSystem::new();
    let logic = Box::new(ReducerLogic::new(
        "counter",
        serde_json::json!({"count": 0}),
        |state, event| {
            let mut s = state.clone();
            if event["type"].as_str() == Some("INC") {
                let c = s["count"].as_i64().unwrap_or(0);
                s.as_object_mut()
                    .unwrap()
                    .insert("count".into(), serde_json::json!(c + 1));
            }
            (s, vec![])
        },
    ));

    let id = system.spawn_logic(logic, "counter".into());
    assert_eq!(system.status(&id), Some(xstate_actor::ActorStatus::Running));

    system.send_to(&id, serde_json::json!({"type": "INC"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["count"], 1);

    system.stop(&id);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(system.status(&id), Some(xstate_actor::ActorStatus::Stopped));
}

#[tokio::test]
async fn multiple_actors_independent() {
    let system = ActorSystem::new();

    let make_logic = || -> Box<dyn ActorLogic> {
        Box::new(ReducerLogic::new(
            "counter",
            serde_json::json!({"count": 0}),
            |state, event| {
                let mut s = state.clone();
                if event["type"].as_str() == Some("INC") {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(c + 1));
                }
                (s, vec![])
            },
        ))
    };

    let id1 = system.spawn_logic(make_logic(), "a".into());
    let id2 = system.spawn_logic(make_logic(), "b".into());
    let id3 = system.spawn_logic(make_logic(), "c".into());

    assert_eq!(system.len(), 3);

    system.send_to(&id1, serde_json::json!({"type": "INC"}));
    system.send_to(&id1, serde_json::json!({"type": "INC"}));
    system.send_to(&id2, serde_json::json!({"type": "INC"}));

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap1 = system.snapshot(&id1).unwrap();
    let snap2 = system.snapshot(&id2).unwrap();
    let snap3 = system.snapshot(&id3).unwrap();

    assert_eq!(snap1["count"], 2);
    assert_eq!(snap2["count"], 1);
    assert_eq!(snap3["count"], 0);
}

#[tokio::test]
async fn stopped_actor_ignores_events() {
    let system = ActorSystem::new();
    let logic = Box::new(ReducerLogic::new(
        "counter",
        serde_json::json!({"count": 0}),
        |state, event| {
            let mut s = state.clone();
            if event["type"].as_str() == Some("INC") {
                let c = s["count"].as_i64().unwrap_or(0);
                s.as_object_mut()
                    .unwrap()
                    .insert("count".into(), serde_json::json!(c + 1));
            }
            (s, vec![])
        },
    ));

    let id = system.spawn_logic(logic, "counter".into());

    system.send_to(&id, serde_json::json!({"type": "INC"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["count"], 1);

    system.stop(&id);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    system.send_to(&id, serde_json::json!({"type": "INC"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["count"], 1, "stopped actor should ignore events");
}

#[tokio::test]
async fn machine_logic_through_system() {
    let system = ActorSystem::new();
    let interp = Interpreter::new(toggle_machine());
    let id = system.spawn(interp, MockClock::new(0), "toggle".into());

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["value"], serde_json::json!("inactive"));

    system.send_to(&id, serde_json::json!({"type": "TOGGLE"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["value"], serde_json::json!("active"));

    system.send_to(&id, serde_json::json!({"type": "TOGGLE"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["value"], serde_json::json!("inactive"));
}

#[tokio::test]
async fn reducer_logic_effects_returned() {
    let system = ActorSystem::new();
    let logic = Box::new(ReducerLogic::new(
        "ping-pong",
        serde_json::json!({"pongs": 0}),
        |state, event| {
            let mut s = state.clone();
            let mut effects = vec![];
            if let Some("PING") = event["type"].as_str() {
                let p = s["pongs"].as_i64().unwrap_or(0);
                s.as_object_mut()
                    .unwrap()
                    .insert("pongs".into(), serde_json::json!(p + 1));
                effects.push(serde_json::json!({"type": "PONG"}));
            }
            (s, effects)
        },
    ));

    let id = system.spawn_logic(logic, "ping-pong".into());
    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["pongs"], 0);

    system.send_to(&id, serde_json::json!({"type": "PING"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["pongs"], 1);
}

#[tokio::test]
async fn parent_stop_cascades_to_children() {
    let system = ActorSystem::new();
    let parent_interp = Interpreter::new(toggle_machine());
    let child_interp = Interpreter::new(toggle_machine());

    let parent_id = system.spawn(parent_interp, MockClock::new(0), "parent".into());
    let child_id = system.spawn_with_parent(
        child_interp,
        MockClock::new(0),
        "child".into(),
        Some("parent".into()),
    );

    assert_eq!(
        system.status(&parent_id),
        Some(xstate_actor::ActorStatus::Running)
    );
    assert_eq!(
        system.status(&child_id),
        Some(xstate_actor::ActorStatus::Running)
    );

    system.stop(&parent_id);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(
        system.status(&parent_id),
        Some(xstate_actor::ActorStatus::Stopped)
    );
    assert_eq!(
        system.status(&child_id),
        Some(xstate_actor::ActorStatus::Stopped)
    );
}

#[tokio::test]
async fn broadcast_reaches_all_actors() {
    let system = ActorSystem::new();

    let make_logic = || -> Box<dyn ActorLogic> {
        Box::new(ReducerLogic::new(
            "counter",
            serde_json::json!({"count": 0}),
            |state, event| {
                let mut s = state.clone();
                if event["type"].as_str() == Some("INC") {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(c + 1));
                }
                (s, vec![])
            },
        ))
    };

    let _id1 = system.spawn_logic(make_logic(), "a".into());
    let _id2 = system.spawn_logic(make_logic(), "b".into());
    let _id3 = system.spawn_logic(make_logic(), "c".into());

    system.broadcast(serde_json::json!({"type": "INC"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap1 = system.snapshot("a").unwrap();
    let snap2 = system.snapshot("b").unwrap();
    let snap3 = system.snapshot("c").unwrap();

    assert_eq!(snap1["count"], 1);
    assert_eq!(snap2["count"], 1);
    assert_eq!(snap3["count"], 1);
}

#[tokio::test]
async fn unknown_event_no_change() {
    let system = ActorSystem::new();
    let logic = Box::new(ReducerLogic::new(
        "counter",
        serde_json::json!({"count": 0}),
        |state, _event| (state.clone(), vec![]),
    ));

    let id = system.spawn_logic(logic, "counter".into());

    system.send_to(&id, serde_json::json!({"type": "UNKNOWN"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["count"], 0);
}
