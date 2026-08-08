use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::mpsc;

use xstate_core::clock::Clock;
use xstate_core::Interpreter;
use xstate_core::StateValue;
use xstate_ir::{StateNode, StateType};

use crate::handle::{ActorHandle, ActorStatus};

fn is_final_state(state: &StateValue, states: &[StateNode]) -> bool {
    match state {
        StateValue::Atomic(name) => states
            .iter()
            .any(|s| s.name == *name && s.node_type == StateType::Final),
        StateValue::Compound {
            state: compound_name,
            child,
        } => {
            let compound = states.iter().find(|s| s.name == *compound_name);
            match compound {
                Some(c) => is_final_state(child, &c.children),
                None => false,
            }
        }
        StateValue::Parallel { regions } => regions.iter().all(|(region_name, region_value)| {
            let region = states.iter().find(|s| s.name == *region_name);
            match region {
                Some(r) => is_final_state(region_value, &r.children),
                None => false,
            }
        }),
    }
}

enum ActorCommand {
    Event(serde_json::Value),
    Stop,
}

pub(crate) struct ActorMessage {
    pub target: String,
    pub event: serde_json::Value,
}

pub(crate) struct SpawnRequest {
    pub parent_id: String,
    pub machine_id: String,
    pub actor_id: String,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_actor(
    interpreter: Interpreter,
    clock: impl Clock + Send + 'static,
    parent_id: String,
    actor_id: String,
    initial_state: Option<StateValue>,
    initial_context: Option<serde_json::Value>,
    on_stop: impl Fn() + Send + 'static,
    outgoing: mpsc::UnboundedSender<ActorMessage>,
    spawn_tx: mpsc::UnboundedSender<SpawnRequest>,
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
                    on_stop();
                    *status_clone.write().unwrap() = ActorStatus::Stopped;
                    break;
                }
                Some(ActorCommand::Event(event)) => {
                    let current_status = *status_clone.read().unwrap();
                    if current_status != ActorStatus::Running {
                        continue;
                    }

                    let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

                    let outgoing_actions = outgoing.clone();
                    let parent_actions = parent_id.clone();
                    let spawn_tx_actions = spawn_tx.clone();
                    let my_id_actions = actor_id.clone();
                    let mut actions =
                        move |name: &str,
                              args: &[serde_json::Value],
                              _event_payload: &serde_json::Value| {
                            match name {
                                "xstate.send_to" => {
                                    let target =
                                        args.first().and_then(|v| v.as_str()).unwrap_or("");
                                    let event =
                                        args.get(1).cloned().unwrap_or(serde_json::Value::Null);
                                    let _ = outgoing_actions.send(ActorMessage {
                                        target: target.to_string(),
                                        event,
                                    });
                                }
                                "xstate.send_parent" => {
                                    let event =
                                        args.first().cloned().unwrap_or(serde_json::Value::Null);
                                    let _ = outgoing_actions.send(ActorMessage {
                                        target: parent_actions.clone(),
                                        event,
                                    });
                                }
                                "xstate.spawn" => {
                                    let machine_id = args
                                        .first()
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let child_actor_id = args
                                        .get(1)
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| format!("{}.child", my_id_actions));
                                    let _ = spawn_tx_actions.send(SpawnRequest {
                                        parent_id: my_id_actions.clone(),
                                        machine_id,
                                        actor_id: child_actor_id,
                                    });
                                }
                                _ => {}
                            }
                        };

                    let new_state = interpreter.send(
                        &current_state,
                        &event,
                        &mut current_context,
                        &guards,
                        &mut actions,
                        &clock,
                    );

                    if let Some(ref new_state) = new_state {
                        if is_final_state(new_state, &interpreter.machine().states) {
                            *status_clone.write().unwrap() = ActorStatus::Done;
                            let done_event = serde_json::json!({
                                "type": format!("done.invoke.{}", actor_id)
                            });
                            let _ = outgoing.send(ActorMessage {
                                target: parent_id.clone(),
                                event: done_event,
                            });
                            break;
                        }
                    }

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
