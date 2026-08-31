use proptest::prelude::*;
use xstate_core::actor_logic::{ActorLogic, ReducerLogic};

proptest! {
    #[test]
    fn reducer_deterministic(
        events in prop::collection::vec(
            prop_oneof![
                Just(serde_json::json!({"type": "INC"})),
                Just(serde_json::json!({"type": "DEC"})),
                Just(serde_json::json!({"type": "RESET"})),
            ],
            0..50
        )
    ) {
        let make_logic = || {
            ReducerLogic::new("counter", serde_json::json!({"count": 0}), |state, event| {
                let mut s = state.clone();
                match event["type"].as_str() {
                    Some("INC") => {
                        let c = s["count"].as_i64().unwrap_or(0);
                        s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(c + 1));
                    }
                    Some("DEC") => {
                        let c = s["count"].as_i64().unwrap_or(0);
                        s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(c - 1));
                    }
                    Some("RESET") => {
                        s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(0));
                    }
                    _ => {}
                }
                (s, vec![])
            })
        };

        let mut logic1 = make_logic();
        let mut snap1 = logic1.initial_snapshot();
        for event in &events {
            logic1.receive(event.clone(), &mut snap1);
        }

        let mut logic2 = make_logic();
        let mut snap2 = logic2.initial_snapshot();
        for event in &events {
            logic2.receive(event.clone(), &mut snap2);
        }

        prop_assert_eq!(snap1, snap2);
    }

    #[test]
    fn snapshot_json_roundtrip(
        state_value in prop_oneof![
            Just(serde_json::json!("idle")),
            Just(serde_json::json!("active")),
            Just(serde_json::json!({"compound": {"child": "leaf"}})),
        ],
        context in prop::collection::hash_map(
            "[a-z]{1,5}",
            prop_oneof![
                Just(serde_json::json!(0)),
                Just(serde_json::json!("")),
                Just(serde_json::json!(true)),
            ],
            0..5
        )
    ) {
        let snap = serde_json::json!({
            "value": state_value,
            "context": context,
        });
        let serialized = serde_json::to_string(&snap).unwrap();
        let deserialized: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        prop_assert_eq!(snap, deserialized);
    }

    #[test]
    fn reducer_never_panics_on_valid_events(
        events in prop::collection::vec(
            prop_oneof![
                Just(serde_json::json!({"type": "INC"})),
                Just(serde_json::json!({"type": "DEC"})),
                Just(serde_json::json!({"type": "RESET"})),
                Just(serde_json::json!({"type": "UNKNOWN"})),
                Just(serde_json::json!({})),
                Just(serde_json::json!(null)),
            ],
            0..100
        )
    ) {
        let mut logic = ReducerLogic::new("counter", serde_json::json!({"count": 0}), |state, event| {
            let mut s = state.clone();
            match event["type"].as_str() {
                Some("INC") => {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(c + 1));
                }
                Some("DEC") => {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(c - 1));
                }
                Some("RESET") => {
                    s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(0));
                }
                _ => {}
            }
            (s, vec![])
        });

        let mut snap = logic.initial_snapshot();
        for event in &events {
            logic.receive(event.clone(), &mut snap);
        }
        prop_assert!(snap["count"].as_i64().is_some(), "count should be an integer");
    }

    #[test]
    fn reducer_count_bounded_by_event_count(
        inc_count in 0usize..100,
        dec_count in 0usize..100,
    ) {
        let mut logic = ReducerLogic::new("counter", serde_json::json!({"count": 0}), |state, event| {
            let mut s = state.clone();
            match event["type"].as_str() {
                Some("INC") => {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(c + 1));
                }
                Some("DEC") => {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(c - 1));
                }
                _ => {}
            }
            (s, vec![])
        });

        let mut snap = logic.initial_snapshot();
        for _ in 0..inc_count {
            logic.receive(serde_json::json!({"type": "INC"}), &mut snap);
        }
        for _ in 0..dec_count {
            logic.receive(serde_json::json!({"type": "DEC"}), &mut snap);
        }

        let final_count = snap["count"].as_i64().unwrap();
        let expected = inc_count as i64 - dec_count as i64;
        prop_assert_eq!(final_count, expected);
    }

    #[test]
    fn effects_always_empty_for_counter(
        events in prop::collection::vec(
            prop_oneof![
                Just(serde_json::json!({"type": "INC"})),
                Just(serde_json::json!({"type": "DEC"})),
                Just(serde_json::json!({"type": "RESET"})),
            ],
            0..50
        )
    ) {
        let mut logic = ReducerLogic::new("counter", serde_json::json!({"count": 0}), |state, event| {
            let mut s = state.clone();
            match event["type"].as_str() {
                Some("INC") => {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(c + 1));
                }
                Some("DEC") => {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(c - 1));
                }
                Some("RESET") => {
                    s.as_object_mut().unwrap().insert("count".into(), serde_json::json!(0));
                }
                _ => {}
            }
            (s, vec![])
        });

        let mut snap = logic.initial_snapshot();
        for event in &events {
            let effects = logic.receive(event.clone(), &mut snap);
            prop_assert!(effects.is_empty(), "counter reducer should not produce effects");
        }
    }
}
