use crate::snapshot::Snapshot;

pub trait Machine {
    type State: Clone + PartialEq;
    type Event;
    type Context: Clone;

    fn initial_state(&self) -> Self::State;
    fn initial_context(&self) -> Self::Context;
    fn transition(
        &self,
        state: &Self::State,
        event: &Self::Event,
        context: &mut Self::Context,
    ) -> Self::State;

    fn start(&self) -> Snapshot<Self::Context, Self::State> {
        Snapshot {
            state_value: self.initial_state(),
            context: self.initial_context(),
            history: Vec::new(),
        }
    }

    fn send(&self, snapshot: &mut Snapshot<Self::Context, Self::State>, event: Self::Event) {
        let previous = snapshot.state_value.clone();
        snapshot.history.push(previous);
        let new_state = self.transition(&snapshot.state_value, &event, &mut snapshot.context);
        snapshot.state_value = new_state;
    }
}

pub trait Actions<C> {
    fn dispatch(&mut self, name: &str, context: &mut C);
}

pub trait Guards<C> {
    fn evaluate(&self, name: &str, context: &C) -> bool;
}
