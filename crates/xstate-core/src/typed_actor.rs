//! The typed counterpart of [`ActorLogic`]: snapshots and events are
//! concrete serde values and **raised events keep their type**
//! (`Vec<Self::Event>`) instead of being erased to JSON.
//!
//! The JSON-erased [`ActorLogic`] remains the wire surface for actors,
//! backends, and bindings (WASM/NAPI speak JSON); [`JsonTypedAdapter`]
//! bridges a typed logic onto that surface via serde round-trips, so a
//! typed machine can be embedded anywhere an `ActorLogic` is expected
//! without giving up its types at the core.
//!
//! ```
//! use xstate_core::typed_actor::{JsonTypedAdapter, TypedActorLogic};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Clone, Serialize, Deserialize, PartialEq)]
//! struct Counter { count: u64 }
//!
//! #[derive(Clone, Serialize, Deserialize, PartialEq)]
//! enum CounterEvent { Increment, Decrement }
//!
//! struct CounterLogic;
//!
//! impl TypedActorLogic for CounterLogic {
//!     type Snapshot = Counter;
//!     type Event = CounterEvent;
//!
//!     fn name(&self) -> &str { "counter" }
//!     fn initial_snapshot(&self) -> Self::Snapshot { Counter { count: 0 } }
//!
//!     fn receive(&mut self, event: Self::Event, snapshot: &mut Self::Snapshot) -> Vec<Self::Event> {
//!         match event {
//!             CounterEvent::Increment => snapshot.count += 1,
//!             CounterEvent::Decrement => snapshot.count = snapshot.count.saturating_sub(1),
//!         }
//!         Vec::new()
//!     }
//! }
//!
//! let mut adapter = JsonTypedAdapter::new(CounterLogic);
//! let mut snap = adapter.initial_snapshot();
//! let _ = adapter.receive(serde_json::json!("Increment"), &mut snap);
//! let _ = adapter.receive(serde_json::json!("Increment"), &mut snap);
//! assert_eq!(snap["count"], 2);
//! ```

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::actor_logic::ActorLogic;

/// The typed counterpart of [`ActorLogic`] (crate::actor_logic): the same
/// lifecycle — initial snapshot, `receive` returning raised events — with
/// concrete types instead of `serde_json::Value`.
///
/// Infallible by design, matching `ActorLogic`: actors that can fail wrap
/// their failure into the snapshot or into raised events (e.g. a typed
/// `Event::Failed(Error)` variant), keeping the receive contract total.
pub trait TypedActorLogic: Send + Sync {
    /// Human-readable name for debugging.
    fn name(&self) -> &str;

    /// The actor's snapshot: everything needed to resume it (serde for the
    /// JSON bridge).
    type Snapshot: Clone;

    /// The actor's event vocabulary (serde for the JSON bridge).
    type Event: Clone;

    /// Create the initial typed snapshot.
    fn initial_snapshot(&self) -> Self::Snapshot;

    /// Process a typed event against the snapshot; return RAISED events,
    /// typed (the counterpart of `ActorLogic::receive`'s `Vec<Value>`).
    fn receive(&mut self, event: Self::Event, snapshot: &mut Self::Snapshot) -> Vec<Self::Event>;
}

/// Drives a [`TypedActorLogic`] through the JSON-erased [`ActorLogic`]
/// surface via serde round-trips: snapshots deserialize into the typed
/// value before the event is applied and serialize back after; raised
/// typed events serialize on the way out.
///
/// Failure policy (total contract): an event or snapshot that does not
/// deserialize into the typed shapes is a shape mismatch — the adapter
/// applies nothing and returns no raised events. Malformed input never
/// panics and never partially applies.
pub struct JsonTypedAdapter<T> {
    inner: T,
}

impl<T> JsonTypedAdapter<T> {
    /// Wraps a typed logic for the erased surface.
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Access the wrapped typed logic.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Mutable access to the wrapped typed logic.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Unwraps the adapter.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> ActorLogic for JsonTypedAdapter<T>
where
    T: TypedActorLogic,
    T::Snapshot: Serialize + DeserializeOwned,
    T::Event: Serialize + DeserializeOwned,
{
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn initial_snapshot(&self) -> Value {
        // Serialization of a typed snapshot can only fail on exotic shapes
        // (e.g. non-string map keys); Null is the honest erased value in
        // that case — the next receive re-derives from what the driver
        // carries, and no partial state is invented.
        serde_json::to_value(self.inner.initial_snapshot()).unwrap_or(Value::Null)
    }

    fn receive(&mut self, event: Value, snapshot: &mut Value) -> Vec<Value> {
        let typed_event = match serde_json::from_value::<T::Event>(event) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut typed_snapshot = match serde_json::from_value::<T::Snapshot>(snapshot.clone()) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let raised = self.inner.receive(typed_event, &mut typed_snapshot);
        if let Ok(back) = serde_json::to_value(&typed_snapshot) {
            *snapshot = back;
        }
        raised
            .into_iter()
            .map(|e| serde_json::to_value(&e).unwrap_or(Value::Null))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
    struct Counter {
        count: u64,
        started: bool,
    }

    #[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
    enum CounterEvent {
        Start,
        Increment,
        Decrement,
        /// Raised exactly when the counter reaches 10 — the typed
        /// raised-event channel under test.
        Reset,
    }

    /// Raises `Reset` exactly when the counter reaches 10 — the typed
    /// raised-event channel under test.
    struct CounterLogic;

    impl TypedActorLogic for CounterLogic {
        type Snapshot = Counter;
        type Event = CounterEvent;

        fn name(&self) -> &str {
            "counter"
        }

        fn initial_snapshot(&self) -> Self::Snapshot {
            Counter {
                count: 0,
                started: false,
            }
        }

        fn receive(
            &mut self,
            event: Self::Event,
            snapshot: &mut Self::Snapshot,
        ) -> Vec<Self::Event> {
            match event {
                CounterEvent::Start => snapshot.started = true,
                CounterEvent::Increment => {
                    snapshot.count += 1;
                }
                CounterEvent::Decrement => {
                    snapshot.count = snapshot.count.saturating_sub(1);
                }
                // Raised by this logic itself when the counter crosses the
                // threshold; the round trip brings it back as input.
                CounterEvent::Reset => {}
            }
            if snapshot.count >= 10 {
                snapshot.count = 0;
                vec![CounterEvent::Reset]
            } else {
                Vec::new()
            }
        }
    }

    // `Reset` is a distinct raised event the driver must handle — the
    // typed channel makes that contract explicit.
    #[test]
    fn typed_events_round_trip_through_the_erased_surface() {
        let mut adapter = JsonTypedAdapter::new(CounterLogic);
        let mut snap = adapter.initial_snapshot();
        assert_eq!(snap["started"], serde_json::Value::Bool(false));

        let raised = adapter.receive(serde_json::json!("Start"), &mut snap);
        assert!(raised.is_empty());
        assert_eq!(snap["started"], serde_json::Value::Bool(true));

        adapter.receive(serde_json::json!("Increment"), &mut snap);
        adapter.receive(serde_json::json!("Increment"), &mut snap);
        assert_eq!(snap["count"], 2);
    }

    #[test]
    fn raised_events_keep_their_type() {
        let mut adapter = JsonTypedAdapter::new(CounterLogic);
        let mut snap = adapter.initial_snapshot();
        adapter.receive(serde_json::json!("Start"), &mut snap);
        for _ in 0..9 {
            let raised = adapter.receive(serde_json::json!("Increment"), &mut snap);
            assert!(raised.is_empty());
        }
        // The 10th increment crosses the threshold: the RAISED event comes
        // back typed — deserializable into the concrete enum.
        let raised = adapter.receive(serde_json::json!("Increment"), &mut snap);
        assert_eq!(raised.len(), 1);
        let typed: CounterEvent = serde_json::from_value(raised[0].clone()).expect("typed raise");
        assert_eq!(typed, CounterEvent::Reset);
        assert_eq!(snap["count"], 0);
    }

    #[test]
    fn shape_mismatches_never_panic_or_partially_apply() {
        let mut adapter = JsonTypedAdapter::new(CounterLogic);
        let mut snap = serde_json::json!({"not": "a counter"});
        let raised = adapter.receive(serde_json::json!("Increment"), &mut snap);
        assert!(raised.is_empty());
        assert_eq!(snap, serde_json::json!({"not": "a counter"}));
    }

    #[test]
    fn saturating_decrement_stays_total() {
        let mut adapter = JsonTypedAdapter::new(CounterLogic);
        let mut snap = adapter.initial_snapshot();
        adapter.receive(serde_json::json!("Decrement"), &mut snap);
        assert_eq!(snap["count"], 0);
    }
}
