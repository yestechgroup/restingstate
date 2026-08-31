use crate::actor_logic::ActorLogic;

/// Status of an actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ActorStatus {
    /// Actor is running and accepting events.
    Running,
    /// Actor has reached a final state and stopped.
    Done,
    /// Actor was stopped externally.
    Stopped,
    /// Actor encountered an error.
    Error,
}

/// Backend-agnostic actor handle trait.
pub trait ActorHandle: Send + Sync {
    /// Send an event to this actor.
    fn send(&self, event: serde_json::Value);
    /// Request the actor to stop.
    fn stop(&self);
    /// Get the current snapshot as JSON: `{ value, context }`.
    fn snapshot(&self) -> serde_json::Value;
    /// Get the actor's current status.
    fn status(&self) -> ActorStatus;
}

/// Backend-agnostic actor system trait.
pub trait ActorSystem: Send + Sync {
    /// The handle type returned by `spawn`.
    type Handle: ActorHandle;

    /// Spawn a new actor with the given logic and ID.
    fn spawn(&self, id: &str, logic: Box<dyn ActorLogic>) -> Self::Handle;
    /// Send an event to a named actor.
    fn send_to(&self, id: &str, event: serde_json::Value);
    /// Stop a named actor.
    fn stop(&self, id: &str);
    /// Get a snapshot of a named actor's current state.
    fn snapshot(&self, id: &str) -> Option<serde_json::Value>;
    /// Get an actor's status.
    fn status(&self, id: &str) -> Option<ActorStatus>;
    /// Number of registered actors.
    fn len(&self) -> usize;
    /// Whether the registry is empty.
    fn is_empty(&self) -> bool;
}
