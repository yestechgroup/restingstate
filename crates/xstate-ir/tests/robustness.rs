// IR deserialize-mutation robustness matrix — issue #29.
use xstate_ir::Machine;

fn base_machine_json() -> serde_json::Value {
    serde_json::json!({
        "id": "toggle",
        "initial": "inactive",
        "context": {
            "fields": [
                { "name": "count", "type_name": "number", "default_value": 0 }
            ]
        },
        "states": [
            {
                "name": "inactive",
                "transitions": [
                    { "event": "toggle", "target": "active", "actions": [], "targets": [] }
                ],
                "children": [],
                "entry": [],
                "exit": [],
                "tags": [],
                "on_done": []
            },
            {
                "name": "active",
                "transitions": [
                    { "event": "toggle", "target": "inactive", "actions": [], "targets": [] }
                ],
                "children": [],
                "entry": [],
                "exit": [],
                "tags": [],
                "on_done": []
            }
        ]
    })
}

fn deserialize(json: &serde_json::Value) -> Result<Machine, String> {
    serde_json::from_value(json.clone()).map_err(|e| e.to_string())
}

fn with_field_removed(base: &serde_json::Value, path: &[&str]) -> serde_json::Value {
    let mut mutated = base.clone();
    let mut current = &mut mutated;
    for (i, key) in path.iter().enumerate() {
        let is_last = i == path.len() - 1;
        if is_last {
            current.as_object_mut().unwrap().remove(*key);
        } else {
            current = current.get_mut(*key).unwrap();
        }
    }
    mutated
}

#[test]
fn missing_required_fields_are_typed_errors() {
    let base = base_machine_json();
    for path in [
        vec!["id"],
        vec!["initial"],
        vec!["context"],
        vec!["states"],
        vec!["context", "fields"],
        vec!["states"],
    ] {
        let mutated = with_field_removed(&base, &path);
        let result = deserialize(&mutated);
        assert!(result.is_err(), "removing {path:?} must be a typed Err");
    }
}

#[test]
fn wrong_types_are_typed_errors() {
    let base = base_machine_json();

    let mut m = base.clone();
    m["id"] = serde_json::json!(123);
    assert!(deserialize(&m).is_err(), "numeric id must be a typed Err");

    let mut m = base.clone();
    m["initial"] = serde_json::json!(null);
    assert!(deserialize(&m).is_err());

    let mut m = base.clone();
    m["states"] = serde_json::json!({});
    assert!(deserialize(&m).is_err(), "object where array expected");

    let mut m = base.clone();
    m["context"] = serde_json::json!([]);
    assert!(deserialize(&m).is_err());

    let mut m = base.clone();
    m["states"][0]["transitions"][0]["delay_ms"] = serde_json::json!("soon");
    assert!(deserialize(&m).is_err(), "string where u64 expected");

    let mut m = base.clone();
    m["states"][0]["transitions"][0]["target"] = serde_json::json!({"deep": true});
    assert!(deserialize(&m).is_err());

    let mut m = base.clone();
    m["states"][0]["children"] = serde_json::json!({});
    assert!(deserialize(&m).is_err());
}

#[test]
fn duplicate_state_names_deserialize_without_panic() {
    let mut m = base_machine_json();
    let duplicate = m["states"][0].clone();
    m["states"].as_array_mut().unwrap().push(duplicate);
    let machine = deserialize(&m).expect("duplicate names are structurally valid IR");
    assert_eq!(machine.states.len(), 3);
}

#[test]
fn initial_pointing_at_nonexistent_child_deserializes() {
    // Structurally valid IR; semantic validation is the runtime's job (#29).
    let mut m = base_machine_json();
    m["states"][0]["initial"] = serde_json::json!("ghost");
    assert!(deserialize(&m).is_ok());
}

#[test]
fn target_of_nonexistent_state_deserializes() {
    let mut m = base_machine_json();
    m["states"][0]["transitions"][0]["target"] = serde_json::json!("nowhere");
    assert!(deserialize(&m).is_ok());
}

#[test]
fn empty_id_and_src_strings_deserialize() {
    let mut m = base_machine_json();
    m["id"] = serde_json::json!("");
    assert!(deserialize(&m).is_ok());

    let mut m = base_machine_json();
    m["states"][0]["invoke"] = serde_json::json!({
        "src": "",
        "on_done": [],
        "on_error": []
    });
    assert!(deserialize(&m).is_ok());
}

#[test]
fn negative_delay_ms_is_typed_error() {
    let mut m = base_machine_json();
    m["states"][0]["transitions"][0]["delay_ms"] = serde_json::json!(-1);
    let err = deserialize(&m).expect_err("negative delay_ms must not deserialize into u64");
    assert!(err.contains("invalid value") || err.contains("-1"), "{err}");
}

#[test]
fn delay_ms_u64_max_deserializes() {
    let mut m = base_machine_json();
    m["states"][0]["transitions"][0]["delay_ms"] = serde_json::json!(u64::MAX);
    assert!(deserialize(&m).is_ok(), "u64::MAX is a valid u64");
}

#[test]
fn duplicate_transition_events_deserialize() {
    let mut m = base_machine_json();
    let t = m["states"][0]["transitions"][0].clone();
    m["states"][0]["transitions"]
        .as_array_mut()
        .unwrap()
        .push(t);
    let machine = deserialize(&m).expect("duplicate events are structurally valid IR");
    assert_eq!(machine.states[0].transitions.len(), 2);
}

#[test]
fn malformed_assign_ops_deserialize_without_panic() {
    // $assign_ops is opaque JSON at the IR layer; the interpreter must also
    // handle these without panicking (covered in xstate-core tests) (#29).
    for ops in [
        serde_json::json!({ "$assign_ops": "not-an-array" }),
        serde_json::json!({ "$assign_ops": [{ "op": "teleport", "field": "count" }] }),
        serde_json::json!({ "$assign_ops": [{ "op": "set", "field": "count", "from": 42 }] }),
        serde_json::json!({ "$assign_ops": [{ "op": "set", "field": "count", "from": "nodot" }] }),
        serde_json::json!({ "$assign_ops": null }),
    ] {
        let mut m = base_machine_json();
        m["states"][0]["transitions"][0]["actions"] = serde_json::json!([
            { "name": "assign", "args": [ops] }
        ]);
        assert!(deserialize(&m).is_ok(), "assign args are opaque JSON");
    }
}

#[test]
fn deeply_nested_context_defaults_are_typed_errors_not_stack_overflow() {
    // A 10k-deep default arrives as JSON text; deserialization hits
    // serde_json's recursion limit and must fail with a typed error, not
    // blow the stack (#29). The deep input is built as a flat string because
    // Value-level serialization is itself recursive.
    let depth = 10_000;
    let mut nested = String::from("0");
    for _ in 0..depth {
        nested = format!("[{nested}]");
    }
    let json = String::from(
        r#"{"id":"toggle","initial":"inactive","context":{"fields":[{"name":"count","type_name":"number","default_value":"#,
    ) + &nested
        + r#"}]}],"states":[{"name":"inactive"},{"name":"active"}]}"#;

    let result = serde_json::from_str::<Machine>(&json);
    let err = result.expect_err("10k-deep context defaults must be a typed Err");
    println!("recursion-limit error: {err}");
}

// Deterministic seeded xorshift64* JSON mutation fuzz — no external deps (#29).
struct Xorshift(u64);

impl Xorshift {
    fn new(seed: u64) -> Self {
        Xorshift(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn seeded_json_mutation_fuzz_never_panics() {
    let seed: u64 = 0x0000_0029_0000_0002;
    let mut rng = Xorshift::new(seed);
    let original = serde_json::to_string(&base_machine_json()).unwrap();

    for _ in 0..500 {
        let mut mutated = original.as_bytes().to_vec();
        let edits = 1 + rng.below(10);
        for _ in 0..edits {
            if mutated.is_empty() {
                break;
            }
            let pos = rng.below(mutated.len());
            match rng.below(4) {
                0 => {
                    mutated.remove(pos);
                }
                1 => {
                    mutated[pos] = (rng.below(256)) as u8;
                }
                2 => {
                    mutated.insert(pos, (rng.below(256)) as u8);
                }
                _ => {
                    let end = (pos + 8).min(mutated.len());
                    let chunk: Vec<u8> = mutated[pos..end].to_vec();
                    mutated.splice(pos..pos, chunk);
                }
            }
        }
        let json = String::from_utf8_lossy(&mutated).into_owned();
        // Ok or Err are both fine; a panic is the bug (#29).
        let _ = serde_json::from_str::<Machine>(&json);
    }
    println!("seeded IR fuzz completed with seed {seed:#x}");
}
