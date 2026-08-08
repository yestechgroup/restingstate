use xstate_ir::StateType;
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
