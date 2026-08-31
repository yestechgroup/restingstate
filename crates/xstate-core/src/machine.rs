use crate::snapshot::Snapshot;

pub trait Machine {
    type State: Clone + PartialEq;
    type Event;
    type EventPayload;
    type Context: Clone;

    fn initial_state(&self) -> Self::State;
    fn initial_context(&self) -> Self::Context;
    fn transition(
        &self,
        state: &Self::State,
        event: &Self::Event,
        payload: &Self::EventPayload,
        context: &mut Self::Context,
    ) -> Self::State;

    fn start(&self) -> Snapshot<Self::Context, Self::State> {
        Snapshot {
            state_value: self.initial_state(),
            context: self.initial_context(),
            history: Vec::new(),
            output: None,
        }
    }

    fn send(
        &self,
        snapshot: &mut Snapshot<Self::Context, Self::State>,
        event: Self::Event,
        payload: Self::EventPayload,
    ) {
        let previous = snapshot.state_value.clone();
        snapshot.history.push(previous);
        let new_state = self.transition(
            &snapshot.state_value,
            &event,
            &payload,
            &mut snapshot.context,
        );
        snapshot.state_value = new_state;
    }
}

pub trait Actions<C> {
    fn dispatch(&mut self, name: &str, context: &mut C, event_payload: &serde_json::Value);
}

pub trait Guards<C> {
    fn evaluate(&self, name: &str, context: &C, event_payload: &serde_json::Value) -> bool;
}
