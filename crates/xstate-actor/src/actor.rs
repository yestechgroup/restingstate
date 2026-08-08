use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::mpsc;

use xstate_core::clock::Clock;
use xstate_core::Interpreter;
use xstate_core::StateValue;

use crate::handle::{ActorHandle, ActorStatus};

enum ActorCommand {
    Event(serde_json::Value),
    Stop,
}

pub(crate) fn spawn_actor(
    interpreter: Interpreter,
    clock: impl Clock + Send + 'static,
    _parent_id: String,
    actor_id: String,
    initial_state: Option<StateValue>,
    initial_context: Option<serde_json::Value>,
) -> Box<dyn ActorHandle> {
    let state = initial_state.unwrap_or_else(|| interpreter.initial_state());
    let context = initial_context.unwrap_or_else(|| interpreter.initial_context());

    let state = Arc::new(RwLock::new(state));
    let context = Arc::new(RwLock::new(context));
    let status = Arc::new(RwLock::new(ActorStatus::Running));

    let (tx, mut rx) = mpsc::unbounded_channel::<ActorCommand>();

    let state_clone = state.clone();
    let context_clone = context.clone();
    let status_clone = status.clone();
    let _done_id = format!("done.invoke.{}", actor_id);
    let _error_id = format!("error.platform.{}", actor_id);

    tokio::spawn(async move {
        let mut current_state = state_clone.read().unwrap().clone();
        let mut current_context = context_clone.read().unwrap().clone();

        loop {
            let cmd = rx.recv().await;
            match cmd {
                Some(ActorCommand::Stop) => {
                    *status_clone.write().unwrap() = ActorStatus::Stopped;
                    break;
                }
                Some(ActorCommand::Event(event)) => {
                    let current_status = *status_clone.read().unwrap();
                    if current_status != ActorStatus::Running {
                        continue;
                    }

                    let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
                    let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

                    let new_state = interpreter.send(
                        &current_state,
                        &event,
                        &mut current_context,
                        &guards,
                        &mut actions,
                        &clock,
                    );

                    if let Some(s) = new_state {
                        current_state = s;
                        *state_clone.write().unwrap() = current_state.clone();
                        *context_clone.write().unwrap() = current_context.clone();
                    }
                }
                None => break,
            }
        }
    });

    Box::new(ActorHandleImpl {
        state,
        context,
        sender: tx,
        status,
    })
}

struct ActorHandleImpl {
    state: Arc<RwLock<StateValue>>,
    context: Arc<RwLock<serde_json::Value>>,
    sender: mpsc::UnboundedSender<ActorCommand>,
    status: Arc<RwLock<ActorStatus>>,
}

impl ActorHandle for ActorHandleImpl {
    fn send(&self, event: serde_json::Value) {
        let _ = self.sender.send(ActorCommand::Event(event));
    }

    fn stop(&self) {
        let _ = self.sender.send(ActorCommand::Stop);
    }

    fn snapshot(&self) -> serde_json::Value {
        let state = self.state.read().unwrap();
        let context = self.context.read().unwrap();
        serde_json::json!({
            "value": serde_json::to_value(&*state).unwrap_or_default(),
            "context": &*context,
        })
    }

    fn status(&self) -> ActorStatus {
        *self.status.read().unwrap()
    }
}
