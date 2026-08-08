use xstate_core::Machine as MachineTrait;

use xstate_macro::xstate;

xstate!("fixtures/toggle.ts");

#[test]
fn toggle_initial_state() {
    let machine = MachineImpl;
    let state = machine.initial_state();
    assert_eq!(state, State::Inactive);
}

#[test]
fn toggle_transition() {
    let machine = MachineImpl;
    let state = State::Inactive;
    let mut context = machine.initial_context();
    let new_state = machine.transition(&state, &Event::Toggle, &mut context);
    assert_eq!(new_state, State::Active);
}

#[test]
fn toggle_round_trip() {
    let machine = MachineImpl;
    let state = State::Inactive;
    let mut context = machine.initial_context();
    let new_state = machine.transition(&state, &Event::Toggle, &mut context);
    assert_eq!(new_state, State::Active);
    let new_state = machine.transition(&new_state, &Event::Toggle, &mut context);
    assert_eq!(new_state, State::Inactive);
}
