use std::env;
use std::fs;
use std::process;

use xstate_core::Interpreter;

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct TestVector {
    machine_name: String,
    machine_ir: xstate_ir::Machine,
    #[serde(default)]
    machine_config: serde_json::Value,
    protocol: Protocol,
    #[serde(default)]
    expected: Vec<ExpectedStep>,
}

#[derive(Debug, serde::Deserialize)]
struct Protocol {
    initial: InitialSnapshot,
    events: Vec<Event>,
}

#[derive(Debug, serde::Deserialize)]
struct InitialSnapshot {
    value: serde_json::Value,
    context: serde_json::Value,
}

#[derive(Debug, serde::Deserialize)]
struct Event {
    #[serde(rename = "type")]
    event_type: String,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct ExpectedStep {
    step: usize,
    event: serde_json::Value,
    snapshot: SnapshotOutput,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct SnapshotOutput {
    value: serde_json::Value,
    context: serde_json::Value,
}

#[derive(Debug, serde::Serialize)]
struct RunnerOutput {
    step: usize,
    event: serde_json::Value,
    snapshot: RunnerSnapshot,
}

#[derive(Debug, serde::Serialize)]
struct RunnerSnapshot {
    value: serde_json::Value,
    context: serde_json::Value,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: rust-runner <test-vector.json>");
        process::exit(1);
    }

    let file_path = &args[1];
    let content = fs::read_to_string(file_path).unwrap_or_else(|e| {
        eprintln!("Error reading file {}: {}", file_path, e);
        process::exit(1);
    });

    let test_vector: TestVector = serde_json::from_str(&content).unwrap_or_else(|e| {
        eprintln!("Error parsing test vector: {}", e);
        process::exit(1);
    });

    let interpreter = Interpreter::new(test_vector.machine_ir);

    let initial_state = interpreter.initial_state();
    let initial_context = interpreter.initial_context();

    let expected_initial: xstate_core::StateValue =
        serde_json::from_value(test_vector.protocol.initial.value.clone()).unwrap_or_else(|e| {
            eprintln!("Warning: could not deserialize initial value: {}", e);
            initial_state.clone()
        });

    if expected_initial != initial_state {
        eprintln!(
            "Warning: initial state mismatch. Expected: {:?}, Got: {:?}",
            expected_initial, initial_state
        );
    }

    if test_vector.protocol.initial.context != initial_context {
        eprintln!(
            "Warning: initial context mismatch. Expected: {}, Got: {}",
            test_vector.protocol.initial.context, initial_context
        );
    }

    let mut state = initial_state;
    let mut context = initial_context;
    let mut outputs: Vec<RunnerOutput> = Vec::new();

    for (i, event) in test_vector.protocol.events.iter().enumerate() {
        let new_state = interpreter.send(
            &state,
            &event.event_type,
            &mut context,
            &|guard_name: &str, ctx: &serde_json::Value| -> bool {
                if let Some(val) = ctx.get(guard_name) {
                    match val {
                        serde_json::Value::Bool(b) => *b,
                        _ => false,
                    }
                } else {
                    true
                }
            },
            &mut |_, _| {},
        );

        if let Some(s) = new_state {
            state = s;
        }

        outputs.push(RunnerOutput {
            step: i,
            event: serde_json::json!({ "type": event.event_type }),
            snapshot: RunnerSnapshot {
                value: serde_json::to_value(&state).expect("Failed to serialize state"),
                context: context.clone(),
            },
        });
    }

    let json_output = serde_json::to_string_pretty(&outputs).expect("Failed to serialize outputs");
    println!("{}", json_output);
}
