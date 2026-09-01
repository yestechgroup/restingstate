use crate::actor_logic::ActorLogic;
use serde::{Deserialize, Serialize};

/// Status of an actor instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActorStatus {
    Running,
    Stopped,
    Error,
    Done,
}

/// Handle to a spawned actor — allows sending events and querying state.
pub trait ActorHandle: Send + Sync {
    /// Send an event to this actor.
    fn send(&self, event: serde_json::Value);
    /// Stop this actor.
    fn stop(&self);
    /// Get the actor's current snapshot.
    fn snapshot(&self) -> serde_json::Value;
    /// Get the actor's current status.
    fn status(&self) -> ActorStatus;
}

/// Backend actor system — manages actor lifecycle.
pub trait ActorSystem: Send + Sync {
    type Handle: ActorHandle;

    /// Spawn a new actor with the given logic.
    fn spawn(&self, id: &str, logic: Box<dyn ActorLogic>) -> Self::Handle;
    /// Send an event to an actor by id.
    fn send_to(&self, id: &str, event: serde_json::Value);
    /// Stop an actor by id.
    fn stop(&self, id: &str);
    /// Get the snapshot of an actor by id.
    fn snapshot(&self, id: &str) -> Option<serde_json::Value>;
    /// Get the status of an actor by id.
    fn status(&self, id: &str) -> Option<ActorStatus>;
    /// Number of actors in the system.
    fn len(&self) -> usize;
    /// Whether the system has no actors.
    fn is_empty(&self) -> bool;
}
