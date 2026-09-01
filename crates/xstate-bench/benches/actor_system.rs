use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use xstate_backend_native::NativeActorSystem;
use xstate_core::actor_logic::{ActorLogic, MachineLogic, ReducerLogic};
use xstate_core::backend::ActorSystem;
use xstate_core::{Interpreter, MockClock};
use xstate_ir::*;

fn default_state(name: &str) -> StateNode {
    StateNode {
        name: name.into(),
        description: None,
        initial: None,
        entry: vec![],
        exit: vec![],
        transitions: vec![],
        children: vec![],
        node_type: StateType::Atomic,
        tags: vec![],
        invoke: None,
        on_done: vec![],
        output: None,
    }
}

fn toggle_machine() -> Machine {
    Machine {
        id: "toggle".into(),
        initial: "inactive".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "inactive".into(),
                transitions: vec![Transition {
                    event: "TOGGLE".into(),
                    target: Some("active".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                ..default_state("inactive")
            },
            StateNode {
                name: "active".into(),
                transitions: vec![Transition {
                    event: "TOGGLE".into(),
                    target: Some("inactive".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                ..default_state("active")
            },
        ],
        setup: None,
    }
}

fn make_logic() -> Box<dyn ActorLogic> {
    let interp = Interpreter::new(toggle_machine());
    Box::new(MachineLogic::new(interp, MockClock::new(0)))
}

fn make_reducer(name: &str) -> Box<dyn ActorLogic> {
    Box::new(ReducerLogic::new(
        name,
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
}

fn bench_spawn(c: &mut Criterion) {
    let mut group = c.benchmark_group("actor_spawn");

    for n in [1, 10, 100, 1000] {
        group.bench_with_input(BenchmarkId::new("spawn_n", n), &n, |b, &n| {
            b.iter_batched(
                || {},
                |_| {
                    let system = NativeActorSystem::new();
                    for i in 0..n {
                        system.spawn(&format!("actor-{i}"), make_logic());
                    }
                    system
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_send(c: &mut Criterion) {
    let mut group = c.benchmark_group("actor_send");

    for n in [1, 10, 100] {
        group.bench_with_input(BenchmarkId::new("send_round_robin", n), &n, |b, &n| {
            let system = NativeActorSystem::new();
            let ids: Vec<String> = (0..n)
                .map(|i| {
                    let id = format!("actor-{i}");
                    system.spawn(&id, make_logic());
                    id
                })
                .collect();
            let event = serde_json::json!({"type": "TOGGLE"});

            b.iter(|| {
                for id in &ids {
                    system.send_to(id, event.clone());
                }
            });
        });
    }
    group.finish();
}

fn bench_stop(c: &mut Criterion) {
    let mut group = c.benchmark_group("actor_stop");

    group.bench_function("stop_single", |b| {
        b.iter_batched(
            || {
                let system = NativeActorSystem::new();
                let _handle = system.spawn("a1", make_logic());
                (system,)
            },
            |(system,)| {
                system.stop("a1");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    for n in [10, 100] {
        group.bench_with_input(BenchmarkId::new("stop_cascade", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let system = NativeActorSystem::new();
                    let _parent_handle = system.spawn("parent", make_logic());
                    for i in 0..n {
                        system.spawn(&format!("child-{i}"), make_logic());
                    }
                    (system,)
                },
                |(system,)| {
                    system.stop("parent");
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("actor_snapshot");

    group.bench_function("snapshot_single", |b| {
        let system = NativeActorSystem::new();
        let _id = system.spawn("a1", make_logic());
        b.iter(|| {
            system.snapshot("a1");
        });
    });

    let system = NativeActorSystem::new();
    for i in 0..100 {
        system.spawn(&format!("actor-{i}"), make_logic());
    }
    group.bench_function("snapshot_100", |b| {
        b.iter(|| {
            for i in 0..100 {
                system.snapshot(&format!("actor-{i}"));
            }
        });
    });
    group.finish();
}

fn bench_reducer(c: &mut Criterion) {
    let mut group = c.benchmark_group("actor_reducer");

    let system = NativeActorSystem::new();
    let _id = system.spawn("reducer", make_reducer("counter"));
    let event = serde_json::json!({"type": "INC"});

    group.bench_function("reducer_send_single", |b| {
        b.iter(|| {
            system.send_to("reducer", event.clone());
        });
    });

    let system = NativeActorSystem::new();
    for i in 0..100 {
        system.spawn(&format!("r-{i}"), make_reducer("counter"));
    }
    group.bench_function("reducer_send_100", |b| {
        b.iter(|| {
            for i in 0..100 {
                system.send_to(&format!("r-{i}"), event.clone());
            }
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_spawn,
    bench_send,
    bench_stop,
    bench_snapshot,
    bench_reducer
);
criterion_main!(benches);
