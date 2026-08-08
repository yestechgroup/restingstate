use serde::{Deserialize, Serialize};

/// Status of an actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Type-erased handle to a spawned actor.
/// All access is through JSON-serialized values for cross-actor compatibility.
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
