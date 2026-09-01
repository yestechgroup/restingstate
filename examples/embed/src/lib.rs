//! Executable walkthrough of docs/embedding.md (issue #33).
//!
//! Every call below is on the stable Interpreter path documented in
//! docs/contracts.md; `run()` asserts instead of printing so this doubles as a
//! workspace test (`cargo test -p embed`).

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;
use xstate_backend_native::NativeActorSystem;
use xstate_core::backend::{ActorHandle, ActorStatus};
use xstate_core::{ActionImpl, ActionRegistry, GuardImpl, GuardRegistry, Interpreter, MockClock};
use xstate_ir::Machine;

const WALLET_TS: &str = r#"
    import { createMachine, assign } from 'xstate';

    export const walletMachine = createMachine({
      id: 'wallet',
      initial: 'open',
      context: { balance: 10, spent: 0 },
      states: {
        open: {
          on: {
            ADD: {
              target: 'open',
              actions: assign({ balance: ({ context }) => context.balance + 20 }),
            },
            SPEND: [
              { target: 'locked', guard: 'canSpend', actions: ['recordSpend'] },
              { target: 'denied' },
            ],
          },
        },
        locked: {
          on: { RETRY: 'open' },
        },
        denied: {
          on: { RETRY: 'open' },
        },
      },
    });
"#;

const RUNNER_TS: &str = r#"
    import { createMachine } from 'xstate';

    export const runnerMachine = createMachine({
      id: 'runner',
      initial: 'idle',
      states: {
        idle: { on: { LOAD: 'loading' } },
        loading: {
          invoke: { src: 'job', onDone: 'completed' },
        },
        completed: {},
      },
    });
"#;

const JOB_TS: &str = r#"
    import { createMachine } from 'xstate';

    export const jobMachine = createMachine({
      id: 'job',
      initial: 'working',
      states: {
        working: { on: { FINISH: { target: 'done', guard: 'canFinish' } } },
        done: { type: 'final' },
      },
    });
"#;

pub fn run() -> Result<(), Box<dyn Error>> {
    // Parse: TypeScript source -> xstate-ir Machine (xstate_parser::parse).
    let machine: Machine = xstate_parser::parse(WALLET_TS)?;
    assert_eq!(machine.id, "wallet");
    assert_eq!(machine.initial, "open");
    assert_eq!(machine.context.fields.len(), 2);

    // Validate gate: machine-IR JSON is the interchange format (docs/contracts.md).
    let machine_json = serde_json::to_string(&machine)?;
    xstate_core::validate_machine_json(&machine_json)?;
    let machine: Machine = serde_json::from_str(&machine_json)?;
    let broken = json!({
        "id": "broken", "initial": "nope", "context": {"fields": []},
        "states": [{"name": "only", "type": "atomic", "transitions": []}],
    });
    assert!(xstate_core::validate_machine_json(&broken.to_string()).is_err());

    // Interpret with GuardRegistry / ActionRegistry.
    let interpreter = Interpreter::new(machine.clone());
    let mut guards = GuardRegistry::new();
    guards.register(GuardImpl::new("canSpend", |ctx, event| {
        ctx["balance"].as_i64().unwrap_or(0) >= event["cost"].as_i64().unwrap_or(i64::MAX)
    }));
    let mut actions = ActionRegistry::new();
    actions.register(ActionImpl::new("recordSpend", |_args, event, ctx| {
        let cost = event["cost"].as_i64().unwrap_or(0);
        let spent = ctx["spent"].as_i64().unwrap_or(0);
        ctx["spent"] = json!(spent + cost);
    }));
    let clock = MockClock::new(0);

    let mut state = interpreter.initial_state();
    let mut ctx = interpreter.initial_context();
    assert_eq!(
        interpreter.snapshot(&state, &ctx),
        json!({"value": "open", "context": {"balance": 10, "spent": 0}})
    );

    // Guard returning false makes the guarded transition fall through to 'denied'.
    let next = interpreter
        .send_with_registry(
            &state,
            &json!({"type": "SPEND", "cost": 25}),
            &mut ctx,
            &guards,
            &actions,
            &clock,
        )
        .expect("fall-through transition");
    state = next;
    assert_eq!(
        interpreter.snapshot(&state, &ctx),
        json!({"value": "denied", "context": {"balance": 10, "spent": 0}})
    );

    state = interpreter
        .send_with_registry(
            &state,
            &json!({"type": "RETRY"}),
            &mut ctx,
            &guards,
            &actions,
            &clock,
        )
        .expect("RETRY");
    state = interpreter
        .send_with_registry(
            &state,
            &json!({"type": "ADD"}),
            &mut ctx,
            &guards,
            &actions,
            &clock,
        )
        .expect("ADD");
    assert_eq!(
        ctx["balance"],
        json!(30),
        "assign runs on the ADD transition"
    );

    // Guard returning true takes the first SPEND arm; its action records the cost.
    state = interpreter
        .send_with_registry(
            &state,
            &json!({"type": "SPEND", "cost": 25}),
            &mut ctx,
            &guards,
            &actions,
            &clock,
        )
        .expect("guarded SPEND");
    assert_eq!(
        interpreter.snapshot(&state, &ctx),
        json!({"value": "locked", "context": {"balance": 30, "spent": 25}})
    );

    // Persist / restore round-trip (versioned envelope, docs/contracts.md).
    let persisted = interpreter.persisted_snapshot(&state, &ctx);
    assert_eq!(persisted["version"], json!(1));
    for key in ["value", "context", "history", "entry_times", "version"] {
        assert!(persisted.get(key).is_some(), "missing key {key}");
    }
    let restorer = Interpreter::new(machine.clone());
    let (restored_state, restored_ctx) = restorer.restore_persisted(&persisted)?;
    assert_eq!(serde_json::to_value(&restored_state)?, json!("locked"));
    assert_eq!(restored_ctx, json!({"balance": 30, "spent": 25}));
    let revived = restorer
        .send_with_registry(
            &restored_state,
            &json!({"type": "RETRY"}),
            &mut restored_ctx.clone(),
            &guards,
            &actions,
            &clock,
        )
        .expect("restored actor keeps processing events");
    assert_eq!(serde_json::to_value(&revived)?, json!("open"));

    // Actor system: invoke parent/child with done-event forwarding.
    let runner = xstate_parser::parse(RUNNER_TS)?;
    let job = xstate_parser::parse(JOB_TS)?;
    xstate_core::validate_machine_json(&serde_json::to_string(&runner)?)?;
    xstate_core::validate_machine_json(&serde_json::to_string(&job)?)?;

    let system = NativeActorSystem::new();
    system.register_machine("job", job);
    let can_finish = Arc::new(AtomicBool::new(false));
    let flag = can_finish.clone();
    system.set_registry_factory(move || {
        let mut guards = GuardRegistry::new();
        let f = flag.clone();
        guards.register(GuardImpl::new("canFinish", move |_, _| {
            f.load(Ordering::SeqCst)
        }));
        (guards, ActionRegistry::new())
    });

    let parent = system.spawn_machine("runner", runner);
    parent.send(json!({"type": "LOAD"}));
    assert_eq!(parent.snapshot()["value"], json!("loading"));
    assert_eq!(
        system.get_status("runner.job"),
        Some(ActorStatus::Running),
        "entering the invoke state auto-spawns the child"
    );
    assert_eq!(system.children_of("runner"), vec!["runner.job".to_string()]);

    // The factory guard blocks FINISH (a missing guard would allow, so this
    // proves the registry is wired into spawned machine actors).
    system.dispatch("runner.job", json!({"type": "FINISH"}));
    assert_eq!(system.get_status("runner.job"), Some(ActorStatus::Running));
    assert_eq!(parent.snapshot()["value"], json!("loading"));

    can_finish.store(true, Ordering::SeqCst);
    system.dispatch("runner.job", json!({"type": "FINISH"}));
    assert_eq!(
        system.get_status("runner.job"),
        Some(ActorStatus::Done),
        "child reached its final state"
    );
    assert_eq!(
        parent.snapshot()["value"],
        json!("completed"),
        "parent received done.invoke.runner.job"
    );

    Ok(())
}
