use xstate_core::Machine as MachineTrait;

use xstate_macro::xstate;

xstate!("fixtures/toggle.ts");

#[test]
fn toggle_from_traffic_light_file() {
    let machine = MachineImpl;
    let state = State::Inactive;
    let mut context = machine.initial_context();
    let new_state = machine.transition(&state, &Event::Toggle, &Default::default(), &mut context);
    assert_eq!(new_state, State::Active);
    assert_eq!(state, State::Inactive);
}
