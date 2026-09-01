use criterion::{Criterion, criterion_group, criterion_main};
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

fn traffic_light_machine() -> Machine {
    Machine {
        id: "traffic_light".into(),
        initial: "green".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "green".into(),
                transitions: vec![Transition {
                    event: "NEXT".into(),
                    target: Some("yellow".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                ..default_state("green")
            },
            StateNode {
                name: "yellow".into(),
                transitions: vec![Transition {
                    event: "NEXT".into(),
                    target: Some("red".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                ..default_state("yellow")
            },
            StateNode {
                name: "red".into(),
                transitions: vec![Transition {
                    event: "NEXT".into(),
                    target: Some("green".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                ..default_state("red")
            },
        ],
        setup: None,
    }
}

fn compound_machine() -> Machine {
    Machine {
        id: "compound".into(),
        initial: "active".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![
            StateNode {
                name: "active".into(),
                initial: Some("running".into()),
                node_type: StateType::Compound,
                transitions: vec![Transition {
                    event: "STOP".into(),
                    target: Some("inactive".into()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                children: vec![
                    StateNode {
                        name: "running".into(),
                        transitions: vec![Transition {
                            event: "PAUSE".into(),
                            target: Some("paused".into()),
                            guard: None,
                            actions: vec![],
                            description: None,
                            delay_ms: None,
                            targets: vec![],
                        }],
                        ..default_state("running")
                    },
                    StateNode {
                        name: "paused".into(),
                        transitions: vec![Transition {
                            event: "RESUME".into(),
                            target: Some("running".into()),
                            guard: None,
                            actions: vec![],
                            description: None,
                            delay_ms: None,
                            targets: vec![],
                        }],
                        ..default_state("paused")
                    },
                ],
                description: None,
                entry: vec![],
                exit: vec![],
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
            },
            StateNode {
                name: "inactive".into(),
                ..default_state("inactive")
            },
        ],
        setup: None,
    }
}

fn parallel_machine() -> Machine {
    let make_region = |name: &str| -> StateNode {
        StateNode {
            name: name.into(),
            initial: Some("off".into()),
            node_type: StateType::Compound,
            transitions: vec![],
            children: vec![
                StateNode {
                    name: "off".into(),
                    transitions: vec![Transition {
                        event: "TOGGLE".into(),
                        target: Some("on".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    }],
                    ..default_state("off")
                },
                StateNode {
                    name: "on".into(),
                    transitions: vec![Transition {
                        event: "TOGGLE".into(),
                        target: Some("off".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    }],
                    ..default_state("on")
                },
            ],
            ..default_state(name)
        }
    };

    Machine {
        id: "parallel".into(),
        initial: "root".into(),
        context: ContextSchema { fields: vec![] },
        states: vec![StateNode {
            name: "root".into(),
            node_type: StateType::Parallel,
            transitions: vec![],
            children: vec![
                make_region("bold"),
                make_region("italic"),
                make_region("underline"),
                make_region("list"),
            ],
            ..default_state("root")
        }],
        setup: None,
    }
}

fn guarded_machine() -> Machine {
    Machine {
        id: "guarded".into(),
        initial: "idle".into(),
        context: ContextSchema {
            fields: vec![ContextField {
                name: "count".into(),
                type_name: "number".into(),
                default_value: serde_json::json!(0),
            }],
        },
        states: vec![
            StateNode {
                name: "idle".into(),
                transitions: vec![Transition {
                    event: "SUBMIT".into(),
                    target: Some("active".into()),
                    guard: Some(Guard {
                        name: "isValid".into(),
                    }),
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                ..default_state("idle")
            },
            StateNode {
                name: "active".into(),
                transitions: vec![Transition {
                    event: "RESET".into(),
                    target: Some("idle".into()),
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

fn actions_machine() -> Machine {
    Machine {
        id: "actions".into(),
        initial: "idle".into(),
        context: ContextSchema {
            fields: vec![ContextField {
                name: "count".into(),
                type_name: "number".into(),
                default_value: serde_json::json!(0),
            }],
        },
        states: vec![
            StateNode {
                name: "idle".into(),
                entry: vec!["logEntry".into()],
                exit: vec!["logExit".into()],
                transitions: vec![Transition {
                    event: "GO".into(),
                    target: Some("active".into()),
                    guard: None,
                    actions: vec![Action {
                        name: "assign".into(),
                        args: vec![serde_json::json!("count"), serde_json::json!(1)],
                    }],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }],
                ..default_state("idle")
            },
            StateNode {
                name: "active".into(),
                entry: vec!["logEntry".into()],
                exit: vec!["logExit".into()],
                transitions: vec![Transition {
                    event: "BACK".into(),
                    target: Some("idle".into()),
                    guard: None,
                    actions: vec![Action {
                        name: "assign".into(),
                        args: vec![serde_json::json!("count"), serde_json::json!(0)],
                    }],
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

fn bench_interpreter(c: &mut Criterion) {
    let mut group = c.benchmark_group("interpreter");

    let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
    let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

    let machine = toggle_machine();
    let interp = Interpreter::new(machine);
    let clock = MockClock::new(0);
    let event = serde_json::json!({"type": "TOGGLE"});
    group.bench_function("toggle", |b| {
        let mut state = interp.initial_state();
        let mut context = interp.initial_context();
        b.iter(|| {
            let new = interp.send(&state, &event, &mut context, &guards, &mut actions, &clock);
            if let Some(s) = new {
                state = s;
            }
        });
    });

    let machine = traffic_light_machine();
    let interp = Interpreter::new(machine);
    let clock = MockClock::new(0);
    let event = serde_json::json!({"type": "NEXT"});
    let mut state = interp.initial_state();
    let mut context = interp.initial_context();
    group.bench_function("traffic_light", |b| {
        b.iter(|| {
            let new = interp.send(&state, &event, &mut context, &guards, &mut actions, &clock);
            if let Some(s) = new {
                state = s;
            }
        });
    });

    let machine = compound_machine();
    let interp = Interpreter::new(machine);
    let clock = MockClock::new(0);
    let event = serde_json::json!({"type": "PAUSE"});
    let mut state = interp.initial_state();
    let mut context = interp.initial_context();
    group.bench_function("compound_pause", |b| {
        b.iter(|| {
            let new = interp.send(&state, &event, &mut context, &guards, &mut actions, &clock);
            if let Some(s) = new {
                state = s;
            }
        });
    });

    let machine = parallel_machine();
    let interp = Interpreter::new(machine);
    let clock = MockClock::new(0);
    let event = serde_json::json!({"type": "TOGGLE"});
    let mut state = interp.initial_state();
    let mut context = interp.initial_context();
    group.bench_function("parallel_toggle", |b| {
        b.iter(|| {
            let new = interp.send(&state, &event, &mut context, &guards, &mut actions, &clock);
            if let Some(s) = new {
                state = s;
            }
        });
    });

    let machine = guarded_machine();
    let interp = Interpreter::new(machine);
    let clock = MockClock::new(0);
    let event = serde_json::json!({"type": "SUBMIT"});
    let mut state = interp.initial_state();
    let mut context = interp.initial_context();
    group.bench_function("guarded_transition", |b| {
        b.iter(|| {
            let new = interp.send(&state, &event, &mut context, &guards, &mut actions, &clock);
            if let Some(s) = new {
                state = s;
            }
        });
    });

    let machine = actions_machine();
    let interp = Interpreter::new(machine);
    let clock = MockClock::new(0);
    let event = serde_json::json!({"type": "GO"});
    let mut state = interp.initial_state();
    let mut context = interp.initial_context();
    group.bench_function("with_actions", |b| {
        b.iter(|| {
            let new = interp.send(&state, &event, &mut context, &guards, &mut actions, &clock);
            if let Some(s) = new {
                state = s;
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_interpreter);
criterion_main!(benches);
