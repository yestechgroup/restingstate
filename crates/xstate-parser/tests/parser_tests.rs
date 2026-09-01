use xstate_ir::{ActorDef, StateType};
use xstate_parser::parse;

macro_rules! fixture {
    ($name:expr) => {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/",
            $name,
            ".ts"
        ))
        .unwrap()
    };
}

#[test]
fn parse_toggle_machine() {
    let source = fixture!("toggle");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "toggle");
    assert_eq!(machine.initial, "inactive");
    assert_eq!(machine.context.fields.len(), 1);
    assert_eq!(machine.context.fields[0].name, "count");
    assert_eq!(machine.context.fields[0].type_name, "number");
    assert_eq!(
        machine.context.fields[0].default_value,
        serde_json::json!(0)
    );

    assert_eq!(machine.states.len(), 2);

    let inactive = &machine.states[0];
    assert_eq!(inactive.name, "inactive");
    assert_eq!(inactive.node_type, StateType::Atomic);
    assert_eq!(inactive.transitions.len(), 1);
    assert_eq!(inactive.transitions[0].event, "TOGGLE");
    assert_eq!(inactive.transitions[0].target.as_deref(), Some("active"));

    let active = &machine.states[1];
    assert_eq!(active.name, "active");
    assert_eq!(active.node_type, StateType::Atomic);
    assert_eq!(active.transitions.len(), 1);
    assert_eq!(active.transitions[0].event, "TOGGLE");
    assert_eq!(active.transitions[0].target.as_deref(), Some("inactive"));
}

#[test]
fn parse_traffic_light_machine() {
    let source = fixture!("traffic-light");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "trafficLight");
    assert_eq!(machine.initial, "red");
    assert!(machine.context.fields.is_empty());

    assert_eq!(machine.states.len(), 3);

    let green = machine.states.iter().find(|s| s.name == "green").unwrap();
    assert_eq!(green.node_type, StateType::Atomic);
    assert_eq!(green.transitions.len(), 1);
    assert_eq!(green.transitions[0].event, "TIMER");
    assert_eq!(green.transitions[0].target.as_deref(), Some("yellow"));

    let yellow = machine.states.iter().find(|s| s.name == "yellow").unwrap();
    assert_eq!(yellow.node_type, StateType::Atomic);
    assert_eq!(yellow.transitions.len(), 1);
    assert_eq!(yellow.transitions[0].event, "TIMER");
    assert_eq!(yellow.transitions[0].target.as_deref(), Some("red"));

    let red = machine.states.iter().find(|s| s.name == "red").unwrap();
    assert_eq!(red.node_type, StateType::Compound);
    assert_eq!(red.initial.as_deref(), Some("walk"));
    assert_eq!(red.transitions.len(), 1);
    assert_eq!(red.transitions[0].event, "TIMER");
    assert_eq!(red.transitions[0].target.as_deref(), Some("green"));

    assert_eq!(red.children.len(), 2);
    let walk = red.children.iter().find(|s| s.name == "walk").unwrap();
    assert_eq!(walk.node_type, StateType::Atomic);
    assert_eq!(walk.transitions.len(), 1);
    assert_eq!(walk.transitions[0].event, "RESET");
    assert_eq!(walk.transitions[0].target.as_deref(), Some("stop"));

    let stop = red.children.iter().find(|s| s.name == "stop").unwrap();
    assert_eq!(stop.node_type, StateType::Final);
    assert!(stop.transitions.is_empty());
}

#[test]
fn parse_guards_machine() {
    let source = fixture!("guards");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "auth");
    assert_eq!(machine.initial, "loggedOut");
    assert_eq!(machine.context.fields.len(), 1);
    assert_eq!(machine.context.fields[0].name, "isAdmin");
    assert_eq!(machine.context.fields[0].type_name, "boolean");
    assert_eq!(
        machine.context.fields[0].default_value,
        serde_json::json!(false)
    );

    assert_eq!(machine.states.len(), 3);

    let logged_out = machine
        .states
        .iter()
        .find(|s| s.name == "loggedOut")
        .unwrap();
    assert_eq!(logged_out.node_type, StateType::Atomic);
    assert_eq!(logged_out.transitions.len(), 2);

    let guarded = &logged_out.transitions[0];
    assert_eq!(guarded.event, "LOGIN");
    assert_eq!(guarded.target.as_deref(), Some("admin"));
    assert_eq!(guarded.guard.as_ref().unwrap().name, "isAdmin");

    let fallback = &logged_out.transitions[1];
    assert_eq!(fallback.event, "LOGIN");
    assert_eq!(fallback.target.as_deref(), Some("basic"));
    assert!(fallback.guard.is_none());

    let admin = machine.states.iter().find(|s| s.name == "admin").unwrap();
    assert_eq!(admin.node_type, StateType::Atomic);
    assert_eq!(admin.transitions.len(), 1);
    assert_eq!(admin.transitions[0].event, "LOGOUT");
    assert_eq!(admin.transitions[0].target.as_deref(), Some("loggedOut"));

    let basic = machine.states.iter().find(|s| s.name == "basic").unwrap();
    assert_eq!(basic.node_type, StateType::Atomic);
    assert_eq!(basic.transitions.len(), 1);
    assert_eq!(basic.transitions[0].event, "LOGOUT");
    assert_eq!(basic.transitions[0].target.as_deref(), Some("loggedOut"));
}

#[test]
fn parse_actions_machine() {
    let source = fixture!("actions");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "form");
    assert_eq!(machine.initial, "editing");
    assert!(machine.context.fields.is_empty());

    assert_eq!(machine.states.len(), 2);

    let editing = machine.states.iter().find(|s| s.name == "editing").unwrap();
    assert_eq!(editing.node_type, StateType::Atomic);
    assert_eq!(editing.entry, vec!["focusField"]);
    assert_eq!(editing.exit, vec!["blurField"]);
    assert_eq!(editing.transitions.len(), 1);
    assert_eq!(editing.transitions[0].event, "SUBMIT");
    assert_eq!(editing.transitions[0].target.as_deref(), Some("submitted"));
    assert_eq!(editing.transitions[0].actions.len(), 2);
    assert_eq!(editing.transitions[0].actions[0].name, "validate");
    assert_eq!(editing.transitions[0].actions[1].name, "save");

    let submitted = machine
        .states
        .iter()
        .find(|s| s.name == "submitted")
        .unwrap();
    assert_eq!(submitted.node_type, StateType::Final);
    assert_eq!(submitted.entry, vec!["showSuccess"]);
    assert!(submitted.transitions.is_empty());
}

#[test]
fn parse_setup_machine() {
    let source = fixture!("setup-machine");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "setupMachine");
    assert_eq!(machine.initial, "idle");
    assert!(machine.context.fields.is_empty());

    assert_eq!(machine.states.len(), 2);

    let idle = machine.states.iter().find(|s| s.name == "idle").unwrap();
    assert_eq!(idle.node_type, StateType::Atomic);
    assert_eq!(idle.transitions.len(), 1);
    assert_eq!(idle.transitions[0].event, "START");
    assert_eq!(idle.transitions[0].target.as_deref(), Some("running"));

    let running = machine.states.iter().find(|s| s.name == "running").unwrap();
    assert_eq!(running.node_type, StateType::Atomic);
    assert_eq!(running.transitions.len(), 1);
    assert_eq!(running.transitions[0].event, "STOP");
    assert_eq!(running.transitions[0].target.as_deref(), Some("idle"));
}

#[test]
fn parse_after_machine() {
    let source = fixture!("after");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "afterTest");
    assert_eq!(machine.initial, "green");

    let green = machine.states.iter().find(|s| s.name == "green").unwrap();
    assert_eq!(green.transitions.len(), 1);
    let green_after = &green.transitions[0];
    assert_eq!(green_after.delay_ms, Some(1000));
    assert_eq!(green_after.target.as_deref(), Some("yellow"));
    assert_eq!(green_after.event, "");

    let yellow = machine.states.iter().find(|s| s.name == "yellow").unwrap();
    assert_eq!(yellow.transitions.len(), 1);
    let yellow_after = &yellow.transitions[0];
    assert_eq!(yellow_after.delay_ms, Some(1000));
    assert_eq!(yellow_after.target.as_deref(), Some("red"));
    assert_eq!(yellow_after.event, "");

    let red = machine.states.iter().find(|s| s.name == "red").unwrap();
    assert!(red.transitions.is_empty());
}

#[test]
fn parse_history_shorthand() {
    let source = fixture!("history-shorthand");
    let machine = parse(&source).unwrap();

    let on_state = machine.states.iter().find(|s| s.name == "on").unwrap();
    let hist = on_state.children.iter().find(|s| s.name == "hist").unwrap();
    assert_eq!(hist.node_type, StateType::History);
}

#[test]
fn parse_invoke_machine() {
    let source = fixture!("invoke");
    let machine = parse(&source).unwrap();
    assert_eq!(machine.id, "invokeTest");

    let loading = &machine.states[0];
    assert!(loading.invoke.is_some());
    assert_eq!(loading.invoke.as_ref().unwrap().src, "fetchData");
    assert_eq!(
        loading.invoke.as_ref().unwrap().id.as_deref(),
        Some("loader")
    );
    assert!(!loading.invoke.as_ref().unwrap().on_done.is_empty());
}

#[test]
fn parse_setup_actors() {
    let source = fixture!("setup-actors");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "withActors");
    assert!(machine.setup.is_some());

    let setup = machine.setup.as_ref().unwrap();
    assert_eq!(setup.actors.len(), 2);
    assert!(setup.guards.is_empty());
    assert!(setup.actions.is_empty());
    assert!(setup.delays.is_empty());

    let loader = setup.actors.get("loader").unwrap();
    assert_eq!(
        *loader,
        ActorDef::FromPromise {
            src: "loader".into()
        }
    );

    let timer = setup.actors.get("timer").unwrap();
    assert_eq!(
        *timer,
        ActorDef::FromCallback {
            src: "timer".into()
        }
    );
}

#[test]
fn parse_setup_guards() {
    let source = fixture!("setup-guards");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "withGuards");
    assert!(machine.setup.is_some());

    let setup = machine.setup.as_ref().unwrap();
    assert!(setup.actors.is_empty());
    assert_eq!(setup.guards.len(), 2);
    assert!(setup.actions.is_empty());
    assert!(setup.delays.is_empty());

    let is_valid = setup.guards.get("isValid").unwrap();
    assert_eq!(is_valid.name, "isValid");
    assert!(is_valid.params.is_empty());

    let is_adult = setup.guards.get("isAdult").unwrap();
    assert_eq!(is_adult.name, "isAdult");
}

#[test]
fn parse_setup_actions() {
    let source = fixture!("setup-actions");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "withActions");
    assert!(machine.setup.is_some());

    let setup = machine.setup.as_ref().unwrap();
    assert!(setup.actors.is_empty());
    assert!(setup.guards.is_empty());
    assert_eq!(setup.actions.len(), 2);
    assert!(setup.delays.is_empty());

    let assign_count = setup.actions.get("assignCount").unwrap();
    assert_eq!(assign_count.name, "assignCount");
    assert!(assign_count.params.is_empty());

    let log_event = setup.actions.get("logEvent").unwrap();
    assert_eq!(log_event.name, "logEvent");
}

#[test]
fn parse_setup_full() {
    let source = fixture!("setup-full");
    let machine = parse(&source).unwrap();

    assert_eq!(machine.id, "fullSetup");
    assert!(machine.setup.is_some());

    let setup = machine.setup.as_ref().unwrap();
    assert_eq!(setup.actors.len(), 1);
    assert_eq!(setup.guards.len(), 1);
    assert_eq!(setup.actions.len(), 1);
    assert_eq!(setup.delays.len(), 1);

    let loader = setup.actors.get("loader").unwrap();
    assert_eq!(
        *loader,
        ActorDef::FromPromise {
            src: "loader".into()
        }
    );

    let is_valid = setup.guards.get("isValid").unwrap();
    assert_eq!(is_valid.name, "isValid");

    let assign_count = setup.actions.get("assignCount").unwrap();
    assert_eq!(assign_count.name, "assignCount");

    let timeout = setup.delays.get("timeout").unwrap();
    assert_eq!(*timeout, 2000);
}

fn transition_actions<'a>(machine: &'a xstate_ir::Machine, event: &str) -> &'a [xstate_ir::Action] {
    &machine.states[0]
        .transitions
        .iter()
        .find(|t| t.event == event)
        .unwrap()
        .actions
}

#[test]
fn parse_dynamic_assign_context_arithmetic() {
    let source = r#"
        const m = createMachine({
          id: 'counter',
          initial: 'idle',
          context: { steps: 0 },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ steps: ({ context }) => context.steps + 1 }) },
              },
            },
          },
        });
    "#;
    let machine = parse(source).unwrap();
    let actions = transition_actions(&machine, "STEP");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].name, "assign");
    assert_eq!(
        actions[0].args,
        vec![serde_json::json!({
            "$assign_ops": [
                { "op": "add", "field": "steps", "value": 1 }
            ]
        })]
    );
}

#[test]
fn parse_dynamic_assign_subtraction() {
    let source = r#"
        const m = createMachine({
          id: 'counter',
          initial: 'idle',
          context: { budget: 10 },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ budget: ({ context }) => context.budget - 1 }) },
              },
            },
          },
        });
    "#;
    let machine = parse(source).unwrap();
    let actions = transition_actions(&machine, "STEP");
    assert_eq!(
        actions[0].args,
        vec![serde_json::json!({
            "$assign_ops": [
                { "op": "sub", "field": "budget", "value": 1 }
            ]
        })]
    );
}

#[test]
fn parse_dynamic_assign_from_event() {
    let source = r#"
        const m = createMachine({
          id: 'assignEvent',
          initial: 'idle',
          context: { last: '' },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ last: ({ event }) => event.value }) },
              },
            },
          },
        });
    "#;
    let machine = parse(source).unwrap();
    let actions = transition_actions(&machine, "STEP");
    assert_eq!(
        actions[0].args,
        vec![serde_json::json!({
            "$assign_ops": [
                { "op": "set", "field": "last", "from": "event.value" }
            ]
        })]
    );
}

#[test]
fn parse_dynamic_assign_nested_event_path() {
    let source = r#"
        const m = createMachine({
          id: 'assignNested',
          initial: 'idle',
          context: { name: '' },
          states: {
            idle: {
              on: {
                STEP: {
                  actions: assign({
                    name: ({ context, event }) => event.user.name,
                  }),
                },
              },
            },
          },
        });
    "#;
    let machine = parse(source).unwrap();
    let actions = transition_actions(&machine, "STEP");
    assert_eq!(
        actions[0].args,
        vec![serde_json::json!({
            "$assign_ops": [
                { "op": "set", "field": "name", "from": "event.user.name" }
            ]
        })]
    );
}

#[test]
fn parse_dynamic_assign_from_context_path() {
    let source = r#"
        const m = createMachine({
          id: 'assignCtx',
          initial: 'idle',
          context: { a: 1, b: 0 },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ b: ({ context }) => context.a }) },
              },
            },
          },
        });
    "#;
    let machine = parse(source).unwrap();
    let actions = transition_actions(&machine, "STEP");
    assert_eq!(
        actions[0].args,
        vec![serde_json::json!({
            "$assign_ops": [
                { "op": "set", "field": "b", "from": "context.a" }
            ]
        })]
    );
}

#[test]
fn parse_dynamic_assign_mixed_static_and_dynamic() {
    let source = r#"
        const m = createMachine({
          id: 'assignMixed',
          initial: 'idle',
          context: { count: 0, last: '' },
          states: {
            idle: {
              on: {
                STEP: {
                  actions: assign({
                    count: ({ context }) => context.count + 1,
                    last: 'step',
                  }),
                },
              },
            },
          },
        });
    "#;
    let machine = parse(source).unwrap();
    let actions = transition_actions(&machine, "STEP");
    assert_eq!(
        actions[0].args,
        vec![serde_json::json!({
            "$assign_ops": [
                { "op": "add", "field": "count", "value": 1 },
                { "op": "set", "field": "last", "value": "step" }
            ]
        })]
    );
}

#[test]
fn parse_dynamic_assign_in_actions_array() {
    let source = r#"
        const m = createMachine({
          id: 'assignArray',
          initial: 'idle',
          context: { count: 0 },
          states: {
            idle: {
              on: {
                STEP: {
                  actions: ['log', assign({ count: ({ context }) => context.count + 1 })],
                },
              },
            },
          },
        });
    "#;
    let machine = parse(source).unwrap();
    let actions = transition_actions(&machine, "STEP");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].name, "log");
    assert_eq!(
        actions[1].args,
        vec![serde_json::json!({
            "$assign_ops": [
                { "op": "add", "field": "count", "value": 1 }
            ]
        })]
    );
}

#[test]
fn parse_static_assign_stays_flat() {
    let source = r#"
        const m = createMachine({
          id: 'assignStatic',
          initial: 'idle',
          context: { count: 0, flag: false },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ count: 5, flag: true }) },
              },
            },
          },
        });
    "#;
    let machine = parse(source).unwrap();
    let actions = transition_actions(&machine, "STEP");
    assert_eq!(
        actions[0].args,
        vec![
            serde_json::json!("count"),
            serde_json::json!(5),
            serde_json::json!("flag"),
            serde_json::json!(true),
        ]
    );
}

#[test]
fn parse_dynamic_assign_literal_return_value() {
    let source = r#"
        const m = createMachine({
          id: 'assignLiteral',
          initial: 'idle',
          context: { status: '' },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ status: () => 'done' }) },
              },
            },
          },
        });
    "#;
    let machine = parse(source).unwrap();
    let actions = transition_actions(&machine, "STEP");
    assert_eq!(
        actions[0].args,
        vec![serde_json::json!({
            "$assign_ops": [
                { "op": "set", "field": "status", "value": "done" }
            ]
        })]
    );
}

#[test]
fn reject_dynamic_assign_multiplication() {
    let source = r#"
        const m = createMachine({
          id: 'badMul',
          initial: 'idle',
          context: { a: 1, b: 2 },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ c: ({ context }) => context.a * context.b }) },
              },
            },
          },
        });
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.to_string().contains('*'),
        "error should name the unsupported operator: {}",
        err
    );
}

#[test]
fn reject_dynamic_assign_method_call() {
    let source = r#"
        const m = createMachine({
          id: 'badMethod',
          initial: 'idle',
          context: { name: '' },
          states: {
            idle: {
              on: {
                STEP: {
                  actions: assign({ name: ({ event }) => event.x.toUpperCase() }),
                },
              },
            },
          },
        });
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("method"),
        "error should name the method call: {}",
        err
    );
}

#[test]
fn reject_dynamic_assign_unknown_identifier() {
    let source = r#"
        const m = createMachine({
          id: 'badIdent',
          initial: 'idle',
          context: { count: 0 },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ count: ({ context }) => helper(context.count) }) },
              },
            },
          },
        });
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.to_string().contains("helper"),
        "error should name the unknown identifier: {}",
        err
    );
}

#[test]
fn reject_dynamic_assign_non_destructured_param() {
    let source = r#"
        const m = createMachine({
          id: 'badParam',
          initial: 'idle',
          context: { count: 0 },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ count: (ctx) => ctx.count + 1 }) },
              },
            },
          },
        });
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.to_string().contains("context"),
        "error should mention the expected ({{ context, event }}) form: {}",
        err
    );
}

#[test]
fn reject_dynamic_assign_block_body() {
    let source = r#"
        const m = createMachine({
          id: 'badBlock',
          initial: 'idle',
          context: { count: 0 },
          states: {
            idle: {
              on: {
                STEP: {
                  actions: assign({
                    count: ({ context }) => {
                      return context.count + 1;
                    },
                  }),
                },
              },
            },
          },
        });
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("expression"),
        "error should require an expression body: {}",
        err
    );
}

#[test]
fn reject_dynamic_assign_arithmetic_on_other_field() {
    let source = r#"
        const m = createMachine({
          id: 'badOther',
          initial: 'idle',
          context: { a: 0, b: 0 },
          states: {
            idle: {
              on: {
                STEP: { actions: assign({ b: ({ context }) => context.a + 1 }) },
              },
            },
          },
        });
    "#;
    let err = parse(source).unwrap_err();
    assert!(
        err.to_string().contains("'b'"),
        "error should name the assigned field: {}",
        err
    );
}
