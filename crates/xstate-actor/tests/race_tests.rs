use xstate_actor::ActorSystem;
use xstate_core::Interpreter;
use xstate_core::clock::MockClock;
use xstate_ir::*;

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
async fn concurrent_send_to_multiple_actors() {
    let system = ActorSystem::new();
    let id1 = system.spawn(
        Interpreter::new(toggle_machine()),
        MockClock::new(0),
        "a1".into(),
    );
    let id2 = system.spawn(
        Interpreter::new(toggle_machine()),
        MockClock::new(0),
        "a2".into(),
    );
    let id3 = system.spawn(
        Interpreter::new(toggle_machine()),
        MockClock::new(0),
        "a3".into(),
    );

    for _ in 0..5 {
        system.send_to(&id1, serde_json::json!({"type": "TOGGLE"}));
        system.send_to(&id2, serde_json::json!({"type": "TOGGLE"}));
        system.send_to(&id3, serde_json::json!({"type": "TOGGLE"}));
    }

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let snap1 = system.snapshot(&id1).unwrap();
    assert_eq!(snap1["value"], serde_json::json!("active"));

    let snap2 = system.snapshot(&id2).unwrap();
    assert_eq!(snap2["value"], serde_json::json!("active"));

    let snap3 = system.snapshot(&id3).unwrap();
    assert_eq!(snap3["value"], serde_json::json!("active"));
}

#[tokio::test]
async fn parent_stop_during_child_event() {
    let system = ActorSystem::new();
    let parent_id = system.spawn(
        Interpreter::new(toggle_machine()),
        MockClock::new(0),
        "parent".into(),
    );
    let child_id = system.spawn_with_parent(
        Interpreter::new(toggle_machine()),
        MockClock::new(0),
        "child".into(),
        Some(parent_id.clone()),
    );

    system.send_to(&child_id, serde_json::json!({"type": "TOGGLE"}));
    system.stop(&parent_id);

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    assert_eq!(
        system.status(&child_id),
        Some(xstate_actor::ActorStatus::Stopped)
    );
}

#[tokio::test]
async fn rapid_fire_events_do_not_corrupt_state() {
    let system = ActorSystem::new();
    let id = system.spawn(
        Interpreter::new(toggle_machine()),
        MockClock::new(0),
        "t".into(),
    );

    for _ in 0..100 {
        system.send_to(&id, serde_json::json!({"type": "TOGGLE"}));
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let snap = system.snapshot(&id).unwrap();
    let val = snap["value"].as_str().unwrap();
    assert!(
        val == "active" || val == "inactive",
        "state should be either active or inactive, got: {}",
        val
    );
}

#[tokio::test]
async fn concurrent_spawn_and_stop() {
    let system = ActorSystem::new();

    for i in 0..10 {
        let name = format!("actor_{}", i);
        system.spawn(
            Interpreter::new(toggle_machine()),
            MockClock::new(0),
            name.clone(),
        );
        if i % 2 == 0 {
            system.stop(&name);
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let len = system.len();
    assert!(len >= 5, "at least 5 actors should exist, got {}", len);
}

#[tokio::test]
async fn broadcast_during_processing() {
    let system = ActorSystem::new();

    for i in 0..5 {
        system.spawn(
            Interpreter::new(toggle_machine()),
            MockClock::new(0),
            format!("actor_{}", i),
        );
    }

    for _ in 0..10 {
        system.broadcast(serde_json::json!({"type": "TOGGLE"}));
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    for i in 0..5 {
        let snap = system.snapshot(&format!("actor_{}", i)).unwrap();
        let val = snap["value"].as_str().unwrap();
        assert!(
            val == "active" || val == "inactive",
            "actor_{} should be in valid state, got: {}",
            i,
            val
        );
    }
}

#[tokio::test]
async fn stop_all_concurrent() {
    let system = ActorSystem::new();

    for i in 0..10 {
        system.spawn(
            Interpreter::new(toggle_machine()),
            MockClock::new(0),
            format!("actor_{}", i),
        );
    }

    system.stop_all();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    for i in 0..10 {
        assert_eq!(
            system.status(&format!("actor_{}", i)),
            Some(xstate_actor::ActorStatus::Stopped),
            "actor_{} should be stopped",
            i
        );
    }
}

#[tokio::test]
async fn snapshot_consistency_under_load() {
    let system = ActorSystem::new();
    let id = system.spawn(
        Interpreter::new(toggle_machine()),
        MockClock::new(0),
        "t".into(),
    );

    let mut snapshots = Vec::new();

    for _ in 0..50 {
        system.send_to(&id, serde_json::json!({"type": "TOGGLE"}));
    }

    for _ in 0..20 {
        if let Some(snap) = system.snapshot(&id) {
            let val = snap["value"].as_str().unwrap().to_string();
            snapshots.push(val);
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    for snap in &snapshots {
        assert!(
            snap == "active" || snap == "inactive",
            "every snapshot should be valid, got: {}",
            snap
        );
    }
}

#[tokio::test]
async fn interleaved_send_stop_send() {
    let system = ActorSystem::new();
    let id = system.spawn(
        Interpreter::new(toggle_machine()),
        MockClock::new(0),
        "t".into(),
    );

    system.send_to(&id, serde_json::json!({"type": "TOGGLE"}));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = system.snapshot(&id).unwrap();
    assert_eq!(snap["value"], serde_json::json!("active"));

    system.stop(&id);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(system.status(&id), Some(xstate_actor::ActorStatus::Stopped));
}

#[tokio::test]
async fn multiple_stop_no_panic() {
    let system = ActorSystem::new();
    let id = system.spawn(
        Interpreter::new(toggle_machine()),
        MockClock::new(0),
        "t".into(),
    );

    system.stop(&id);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    system.stop(&id);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(system.status(&id), Some(xstate_actor::ActorStatus::Stopped));
}

#[tokio::test]
async fn send_to_nonexistent_actor_no_panic() {
    let system = ActorSystem::new();
    system.send_to("nonexistent", serde_json::json!({"type": "TOGGLE"}));
    assert!(system.snapshot("nonexistent").is_none());
}

#[tokio::test]
async fn gc_after_rapid_operations() {
    let system = ActorSystem::new();

    for i in 0..20 {
        system.spawn(
            Interpreter::new(toggle_machine()),
            MockClock::new(0),
            format!("actor_{}", i),
        );
    }

    for i in 0..10 {
        system.stop(&format!("actor_{}", i));
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    system.gc();

    assert_eq!(system.len(), 10, "gc should remove 10 stopped actors");
}
