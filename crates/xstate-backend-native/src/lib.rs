use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use xstate_core::StateValue;
use xstate_core::actions::ActionRegistry;
use xstate_core::actor_logic::{ActorLogic, MachineLogic};
use xstate_core::backend::{ActorHandle, ActorStatus, ActorSystem as BackendActorSystem};
use xstate_core::clock::MockClock;
use xstate_core::guards::GuardRegistry;
use xstate_core::interpreter::Interpreter;
use xstate_ir::{Machine, StateNode, StateType, Transition};

type Mailbox = Arc<Mutex<Vec<Effect>>>;
type RegistryFactory = Arc<dyn Fn() -> (GuardRegistry, ActionRegistry) + Send + Sync>;

#[derive(Debug, Clone)]
enum Effect {
    Send {
        target: String,
        event: serde_json::Value,
    },
    Spawn {
        parent: String,
        machine_name: String,
        actor_id: String,
    },
}

struct ActorState {
    logic: Box<dyn ActorLogic>,
    snapshot: serde_json::Value,
    status: ActorStatus,
    parent: Option<String>,
    machine: Option<Machine>,
    mailbox: Option<Mailbox>,
}

struct Inner {
    actors: Mutex<HashMap<String, ActorState>>,
    children: Mutex<HashMap<String, Vec<String>>>,
    machines: Mutex<HashMap<String, Machine>>,
    registry_factory: Mutex<Option<RegistryFactory>>,
}

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

fn find_state_node<'a>(name: &str, states: &'a [StateNode]) -> Option<&'a StateNode> {
    for state in states {
        if state.name == name {
            return Some(state);
        }
        if let Some(found) = find_state_node(name, &state.children) {
            return Some(found);
        }
    }
    None
}

fn top_level_state_name(value: &StateValue) -> Option<String> {
    match value {
        StateValue::Atomic(name) => Some(name.clone()),
        StateValue::Compound { state, .. } => Some(state.clone()),
        StateValue::Parallel { regions } => regions.first().map(|(n, _)| n.clone()),
    }
}

fn rewrite_invoke(states: &mut [StateNode], actor_id: &str) {
    for node in states.iter_mut() {
        if let Some(invoke) = node.invoke.clone() {
            let invoke_id = invoke
                .id
                .clone()
                .unwrap_or_else(|| format!("{actor_id}.{}", invoke.src));
            let done = invoke.on_done.iter().map(|t| Transition {
                event: format!("done.invoke.{invoke_id}"),
                ..t.clone()
            });
            let error = invoke.on_error.iter().map(|t| Transition {
                event: format!("error.platform.{invoke_id}"),
                ..t.clone()
            });
            node.transitions.extend(done);
            node.transitions.extend(error);
        }
        rewrite_invoke(&mut node.children, actor_id);
    }
}

fn prepare_machine(machine: &Machine, actor_id: &str) -> Machine {
    let mut prepared = machine.clone();
    rewrite_invoke(&mut prepared.states, actor_id);
    prepared
}

impl Inner {
    fn machine_registries(
        &self,
        actor_id: &str,
        parent: Option<&str>,
    ) -> (GuardRegistry, ActionRegistry, Mailbox) {
        let factory = self.registry_factory.lock().unwrap().clone();
        let (guards, mut actions) = match factory {
            Some(f) => f(),
            None => (GuardRegistry::new(), ActionRegistry::new()),
        };
        let mailbox: Mailbox = Arc::new(Mutex::new(Vec::new()));

        let send_mailbox = mailbox.clone();
        actions.register(xstate_core::ActionImpl::new(
            "xstate.send_to",
            move |args, _, _| {
                let target = args
                    .first()
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let event = args.get(1).cloned().unwrap_or(serde_json::Value::Null);
                send_mailbox
                    .lock()
                    .unwrap()
                    .push(Effect::Send { target, event });
            },
        ));

        let parent_for_send = parent.map(|p| p.to_string());
        let parent_mailbox = mailbox.clone();
        actions.register(xstate_core::ActionImpl::new(
            "xstate.send_parent",
            move |args, _, _| {
                if let Some(parent) = &parent_for_send {
                    let event = args.first().cloned().unwrap_or(serde_json::Value::Null);
                    parent_mailbox.lock().unwrap().push(Effect::Send {
                        target: parent.clone(),
                        event,
                    });
                }
            },
        ));

        let spawn_actor_id = actor_id.to_string();
        let spawn_mailbox = mailbox.clone();
        actions.register(xstate_core::ActionImpl::new(
            "xstate.spawn",
            move |args, _, _| {
                let machine_name = args
                    .first()
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let child_id = args
                    .get(1)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{}.child", spawn_actor_id));
                spawn_mailbox.lock().unwrap().push(Effect::Spawn {
                    parent: spawn_actor_id.clone(),
                    machine_name,
                    actor_id: child_id,
                });
            },
        ));

        (guards, actions, mailbox)
    }

    fn run(&self, seeds: Vec<(String, serde_json::Value)>) {
        let mut queue: VecDeque<(String, serde_json::Value)> = seeds.into();
        while let Some((id, event)) = queue.pop_front() {
            let effects = {
                let mut actors = self.actors.lock().unwrap();
                let Some(actor) = actors.get_mut(&id) else {
                    continue;
                };
                if actor.status != ActorStatus::Running {
                    continue;
                }
                actor.logic.receive(event, &mut actor.snapshot);

                let mut effects: Vec<Effect> = Vec::new();
                if actor.snapshot.get("status").and_then(|s| s.as_str()) == Some("rejected") {
                    actor.status = ActorStatus::Error;
                    if let Some(parent) = actor.parent.clone() {
                        let error = actor
                            .snapshot
                            .get("error")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        effects.push(Effect::Send {
                            target: parent,
                            event: serde_json::json!({
                                "type": format!("error.platform.{id}"),
                                "error": error,
                            }),
                        });
                    }
                } else if let Some(machine) = actor.machine.clone() {
                    let value: StateValue = serde_json::from_value(actor.snapshot["value"].clone())
                        .unwrap_or(StateValue::Atomic(String::new()));
                    if is_final_state(&value, machine.states.as_slice()) {
                        actor.status = ActorStatus::Done;
                        if let Some(parent) = actor.parent.clone() {
                            let output = actor.snapshot.get("context").cloned().unwrap_or_default();
                            effects.push(Effect::Send {
                                target: parent,
                                event: serde_json::json!({
                                    "type": format!("done.invoke.{id}"),
                                    "output": output,
                                }),
                            });
                        }
                    } else if let Some(name) = top_level_state_name(&value)
                        && let Some(node) = find_state_node(&name, &machine.states)
                        && let Some(invoke) = &node.invoke
                    {
                        let child_id = invoke
                            .id
                            .clone()
                            .unwrap_or_else(|| format!("{id}.{}", invoke.src));
                        effects.push(Effect::Spawn {
                            parent: id.clone(),
                            machine_name: invoke.src.clone(),
                            actor_id: child_id,
                        });
                    }
                }

                if let Some(mailbox) = &actor.mailbox {
                    effects.extend(mailbox.lock().unwrap().drain(..));
                }
                effects
            };

            for effect in effects {
                match effect {
                    Effect::Send { target, event } => queue.push_back((target, event)),
                    Effect::Spawn {
                        parent,
                        machine_name,
                        actor_id,
                    } => self.spawn_invoked_child(&parent, &machine_name, &actor_id),
                }
            }
        }
    }

    fn spawn_invoked_child(&self, parent: &str, machine_name: &str, actor_id: &str) {
        let machine = self.machines.lock().unwrap().get(machine_name).cloned();
        let Some(machine) = machine else {
            return;
        };
        let prepared = prepare_machine(&machine, actor_id);
        let (guards, actions, mailbox) = self.machine_registries(actor_id, Some(parent));
        let interp = Interpreter::new(prepared.clone());
        let logic = MachineLogic::new_with_registries(interp, MockClock::new(0), guards, actions);
        let snapshot = logic.initial_snapshot();
        self.actors.lock().unwrap().insert(
            actor_id.to_string(),
            ActorState {
                logic: Box::new(logic),
                snapshot,
                status: ActorStatus::Running,
                parent: Some(parent.to_string()),
                machine: Some(prepared),
                mailbox: Some(mailbox),
            },
        );
        self.children
            .lock()
            .unwrap()
            .entry(parent.to_string())
            .or_default()
            .push(actor_id.to_string());
    }

    fn stop_cascade(&self, id: &str) {
        let mut actors = self.actors.lock().unwrap();
        if let Some(actor) = actors.get_mut(id) {
            actor.logic.stop();
            actor.status = ActorStatus::Stopped;
        }
        drop(actors);
        let kids: Vec<String> = self
            .children
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default();
        for kid in kids {
            self.stop_cascade(&kid);
        }
    }
}

pub struct NativeActorSystem {
    inner: Arc<Inner>,
}

impl NativeActorSystem {
    pub fn new() -> Self {
        NativeActorSystem {
            inner: Arc::new(Inner {
                actors: Mutex::new(HashMap::new()),
                children: Mutex::new(HashMap::new()),
                machines: Mutex::new(HashMap::new()),
                registry_factory: Mutex::new(None),
            }),
        }
    }

    pub fn register_machine(&self, name: &str, machine: Machine) {
        self.inner
            .machines
            .lock()
            .unwrap()
            .insert(name.to_string(), machine);
    }

    pub fn set_registry_factory(
        &self,
        factory: impl Fn() -> (GuardRegistry, ActionRegistry) + Send + Sync + 'static,
    ) {
        *self.inner.registry_factory.lock().unwrap() = Some(Arc::new(factory));
    }

    pub fn spawn_machine(&self, id: &str, machine: Machine) -> NativeActorHandle {
        self.spawn_machine_inner(id, machine, None)
    }

    pub fn spawn_machine_with_parent(
        &self,
        id: &str,
        machine: Machine,
        parent: &str,
    ) -> NativeActorHandle {
        self.spawn_machine_inner(id, machine, Some(parent.to_string()))
    }

    pub fn spawn_machine_from_persisted(
        &self,
        id: &str,
        machine: Machine,
        persisted: &serde_json::Value,
    ) -> Result<NativeActorHandle, xstate_core::interpreter::PersistError> {
        let prepared = prepare_machine(&machine, id);
        let (guards, actions, mailbox) = self.inner.machine_registries(id, None);
        let interp = Interpreter::new(prepared.clone());
        let logic = MachineLogic::new_with_registries(interp, MockClock::new(0), guards, actions);
        let snapshot = logic.restore_persisted(persisted)?;
        self.inner.actors.lock().unwrap().insert(
            id.to_string(),
            ActorState {
                logic: Box::new(logic),
                snapshot,
                status: ActorStatus::Running,
                parent: None,
                machine: Some(prepared),
                mailbox: Some(mailbox),
            },
        );
        Ok(NativeActorHandle {
            inner: Arc::clone(&self.inner),
            id: id.to_string(),
        })
    }

    pub fn spawn_logic_with_parent(
        &self,
        id: &str,
        logic: Box<dyn ActorLogic>,
        parent: &str,
    ) -> NativeActorHandle {
        let snapshot = logic.initial_snapshot();
        self.inner.actors.lock().unwrap().insert(
            id.to_string(),
            ActorState {
                logic,
                snapshot,
                status: ActorStatus::Running,
                parent: Some(parent.to_string()),
                machine: None,
                mailbox: None,
            },
        );
        self.inner
            .children
            .lock()
            .unwrap()
            .entry(parent.to_string())
            .or_default()
            .push(id.to_string());
        NativeActorHandle {
            inner: Arc::clone(&self.inner),
            id: id.to_string(),
        }
    }

    fn spawn_machine_inner(
        &self,
        id: &str,
        machine: Machine,
        parent: Option<String>,
    ) -> NativeActorHandle {
        let prepared = prepare_machine(&machine, id);
        let (guards, actions, mailbox) = self.inner.machine_registries(id, parent.as_deref());
        let interp = Interpreter::new(prepared.clone());
        let logic = MachineLogic::new_with_registries(interp, MockClock::new(0), guards, actions);
        let snapshot = logic.initial_snapshot();
        self.inner.actors.lock().unwrap().insert(
            id.to_string(),
            ActorState {
                logic: Box::new(logic),
                snapshot,
                status: ActorStatus::Running,
                parent: parent.clone(),
                machine: Some(prepared),
                mailbox: Some(mailbox),
            },
        );
        if let Some(parent) = parent {
            self.inner
                .children
                .lock()
                .unwrap()
                .entry(parent.to_string())
                .or_default()
                .push(id.to_string());
        }
        NativeActorHandle {
            inner: Arc::clone(&self.inner),
            id: id.to_string(),
        }
    }

    pub fn children_of(&self, id: &str) -> Vec<String> {
        self.inner
            .children
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn dispatch(&self, id: &str, event: serde_json::Value) {
        self.inner.run(vec![(id.to_string(), event)]);
    }

    pub fn stop_actor(&self, id: &str) {
        self.inner.stop_cascade(id);
    }

    pub fn get_snapshot(&self, id: &str) -> Option<serde_json::Value> {
        self.inner
            .actors
            .lock()
            .unwrap()
            .get(id)
            .map(|a| a.snapshot.clone())
    }

    pub fn get_status(&self, id: &str) -> Option<ActorStatus> {
        self.inner.actors.lock().unwrap().get(id).map(|a| a.status)
    }
}

impl Default for NativeActorSystem {
    fn default() -> Self {
        Self::new()
    }
}

pub struct NativeActorHandle {
    inner: Arc<Inner>,
    id: String,
}

impl ActorHandle for NativeActorHandle {
    fn send(&self, event: serde_json::Value) {
        self.inner.run(vec![(self.id.clone(), event)]);
    }

    fn stop(&self) {
        self.inner.stop_cascade(&self.id);
    }

    fn snapshot(&self) -> serde_json::Value {
        self.inner
            .actors
            .lock()
            .unwrap()
            .get(&self.id)
            .map(|a| a.snapshot.clone())
            .unwrap_or(serde_json::Value::Null)
    }

    fn status(&self) -> ActorStatus {
        self.inner
            .actors
            .lock()
            .unwrap()
            .get(&self.id)
            .map(|a| a.status)
            .unwrap_or(ActorStatus::Error)
    }
}

impl BackendActorSystem for NativeActorSystem {
    type Handle = NativeActorHandle;

    fn spawn(&self, id: &str, logic: Box<dyn ActorLogic>) -> Self::Handle {
        let snapshot = logic.initial_snapshot();
        let actor = ActorState {
            logic,
            snapshot,
            status: ActorStatus::Running,
            parent: None,
            machine: None,
            mailbox: None,
        };
        self.inner
            .actors
            .lock()
            .unwrap()
            .insert(id.to_string(), actor);
        NativeActorHandle {
            inner: Arc::clone(&self.inner),
            id: id.to_string(),
        }
    }

    fn send_to(&self, id: &str, event: serde_json::Value) {
        self.dispatch(id, event);
    }

    fn stop(&self, id: &str) {
        self.stop_actor(id);
    }

    fn snapshot(&self, id: &str) -> Option<serde_json::Value> {
        self.get_snapshot(id)
    }

    fn status(&self, id: &str) -> Option<ActorStatus> {
        self.get_status(id)
    }

    fn len(&self) -> usize {
        self.inner.actors.lock().unwrap().len()
    }

    fn is_empty(&self) -> bool {
        self.inner.actors.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod invoke_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    use xstate_core::actions::{ActionImpl, ActionRegistry};
    use xstate_core::actor_logic::PromiseLogic;
    use xstate_core::guards::{GuardImpl, GuardRegistry};
    use xstate_ir::{
        Action, ContextSchema, InvokeConfig, Machine, StateNode, StateType, Transition,
    };

    fn trans(event: &str, target: &str) -> Transition {
        Transition {
            event: event.into(),
            target: Some(target.into()),
            guard: None,
            actions: vec![],
            description: None,
            delay_ms: None,
            targets: vec![],
        }
    }

    fn action_trans(event: &str, actions: Vec<Action>) -> Transition {
        Transition {
            event: event.into(),
            target: None,
            guard: None,
            actions,
            description: None,
            delay_ms: None,
            targets: vec![],
        }
    }

    fn atomic(name: &str, transitions: Vec<Transition>) -> StateNode {
        StateNode {
            name: name.into(),
            description: None,
            initial: None,
            entry: vec![],
            exit: vec![],
            transitions,
            children: vec![],
            node_type: StateType::Atomic,
            tags: vec![],
            invoke: None,
            on_done: vec![],
            output: None,
        }
    }

    fn final_node(name: &str) -> StateNode {
        StateNode {
            name: name.into(),
            description: None,
            initial: None,
            entry: vec![],
            exit: vec![],
            transitions: vec![],
            children: vec![],
            node_type: StateType::Final,
            tags: vec![],
            invoke: None,
            on_done: vec![],
            output: None,
        }
    }

    fn machine(id: &str, initial: &str, states: Vec<StateNode>) -> Machine {
        Machine {
            id: id.into(),
            initial: initial.into(),
            context: ContextSchema { fields: vec![] },
            states,
            setup: None,
        }
    }

    fn child_machine() -> Machine {
        machine(
            "child",
            "working",
            vec![
                atomic("working", vec![trans("FINISH", "end")]),
                final_node("end"),
            ],
        )
    }

    fn parent_machine() -> Machine {
        let mut loading = atomic(
            "loading",
            vec![trans("done.invoke.runner.child", "completed")],
        );
        loading.invoke = Some(InvokeConfig {
            src: "child".into(),
            id: None,
            on_done: vec![],
            on_error: vec![],
        });
        machine(
            "runner",
            "idle",
            vec![
                atomic("idle", vec![trans("LOAD", "loading")]),
                loading,
                atomic("completed", vec![]),
            ],
        )
    }

    #[test]
    fn invoke_auto_spawns_child_and_forwards_done_to_parent() {
        let system = NativeActorSystem::new();
        system.register_machine("child", child_machine());
        let parent = system.spawn_machine("runner", parent_machine());

        parent.send(serde_json::json!({"type": "LOAD"}));
        assert_eq!(parent.snapshot()["value"], "loading");
        assert_eq!(
            system.status("runner.child"),
            Some(ActorStatus::Running),
            "entering an invoke state should auto-spawn the child actor"
        );

        system.send_to("runner.child", serde_json::json!({"type": "FINISH"}));
        assert_eq!(
            system.status("runner.child"),
            Some(ActorStatus::Done),
            "child reaching a final state should be marked done"
        );
        assert_eq!(
            system.snapshot("runner").unwrap()["value"],
            "completed",
            "parent should receive done.invoke.runner.child and transition"
        );
    }

    #[test]
    fn invoke_on_done_config_translates_to_done_event_transition() {
        let mut loading = atomic("loading", vec![]);
        loading.invoke = Some(InvokeConfig {
            src: "child".into(),
            id: None,
            on_done: vec![trans("ignored", "completed")],
            on_error: vec![],
        });
        let parent = machine(
            "runner",
            "idle",
            vec![
                atomic("idle", vec![trans("LOAD", "loading")]),
                loading,
                atomic("completed", vec![]),
            ],
        );

        let system = NativeActorSystem::new();
        system.register_machine("child", child_machine());
        let parent = system.spawn_machine("runner", parent);

        parent.send(serde_json::json!({"type": "LOAD"}));
        system.send_to("runner.child", serde_json::json!({"type": "FINISH"}));

        assert_eq!(
            system.snapshot("runner").unwrap()["value"],
            "completed",
            "invoke.onDone should fire when the child finishes"
        );
    }

    #[test]
    fn send_to_convention_routes_event_to_another_actor() {
        let parent = machine(
            "parent",
            "idle",
            vec![atomic(
                "idle",
                vec![action_trans(
                    "TRIGGER",
                    vec![Action {
                        name: "xstate.send_to".into(),
                        args: vec![
                            serde_json::json!("child"),
                            serde_json::json!({"type": "POKE"}),
                        ],
                    }],
                )],
            )],
        );
        let child = machine(
            "child",
            "idle",
            vec![
                atomic("idle", vec![trans("POKE", "poked")]),
                atomic("poked", vec![]),
            ],
        );

        let system = NativeActorSystem::new();
        system.spawn_machine("child", child);
        let parent = system.spawn_machine("parent", parent);

        parent.send(serde_json::json!({"type": "TRIGGER"}));
        assert_eq!(
            system.snapshot("child").unwrap()["value"],
            "poked",
            "xstate.send_to should route the event to the target actor"
        );
    }

    #[test]
    fn send_parent_convention_reaches_the_parent() {
        let parent = machine(
            "parent",
            "idle",
            vec![
                atomic("idle", vec![trans("CHILD_DONE", "done")]),
                atomic("done", vec![]),
            ],
        );
        let child = machine(
            "child",
            "idle",
            vec![atomic(
                "idle",
                vec![action_trans(
                    "PING",
                    vec![Action {
                        name: "xstate.send_parent".into(),
                        args: vec![serde_json::json!({"type": "CHILD_DONE"})],
                    }],
                )],
            )],
        );

        let system = NativeActorSystem::new();
        system.spawn_machine("parent", parent);
        let child = system.spawn_machine_with_parent("child", child, "parent");

        child.send(serde_json::json!({"type": "PING"}));
        assert_eq!(
            system.snapshot("parent").unwrap()["value"],
            "done",
            "xstate.send_parent should deliver the event to the parent actor"
        );
    }

    #[test]
    fn spawn_action_convention_creates_child_from_registered_machine() {
        let parent = machine(
            "spawner",
            "idle",
            vec![atomic(
                "idle",
                vec![action_trans(
                    "SPAWN",
                    vec![Action {
                        name: "xstate.spawn".into(),
                        args: vec![
                            serde_json::json!("child"),
                            serde_json::json!("spawner.child"),
                        ],
                    }],
                )],
            )],
        );

        let system = NativeActorSystem::new();
        system.register_machine("child", child_machine());
        let parent = system.spawn_machine("spawner", parent);
        assert_eq!(system.len(), 1);

        parent.send(serde_json::json!({"type": "SPAWN"}));
        assert_eq!(system.len(), 2);
        assert_eq!(system.status("spawner.child"), Some(ActorStatus::Running));
    }

    #[test]
    fn rejected_promise_child_sends_error_platform_to_parent() {
        let parent = machine(
            "parent",
            "idle",
            vec![
                atomic("idle", vec![trans("error.platform.pchild", "failed")]),
                atomic("failed", vec![]),
            ],
        );

        let system = NativeActorSystem::new();
        system.spawn_machine("parent", parent);
        let child = system.spawn_logic_with_parent(
            "pchild",
            Box::new(PromiseLogic::new("pchild", async {
                Err(serde_json::json!({"message": "boom"}))
            })),
            "parent",
        );

        child.send(serde_json::json!({"type": "POLL"}));
        assert_eq!(
            system.status("pchild"),
            Some(ActorStatus::Error),
            "a rejecting promise child should end in the Error status"
        );
        assert_eq!(
            system.snapshot("parent").unwrap()["value"],
            "failed",
            "parent should receive error.platform.pchild and transition"
        );
    }

    #[test]
    fn registry_factory_guard_blocks_transition_in_spawned_machine() {
        let allowed = Arc::new(AtomicBool::new(false));
        let flag = allowed.clone();
        let system = NativeActorSystem::new();
        system.set_registry_factory(move || {
            let mut guards = GuardRegistry::new();
            let f = flag.clone();
            guards.register(GuardImpl::new("canToggle", move |_, _| {
                f.load(Ordering::SeqCst)
            }));
            (guards, ActionRegistry::new())
        });

        let guarded = machine(
            "guarded",
            "off",
            vec![
                {
                    let mut t = trans("TOGGLE", "on");
                    t.guard = Some(xstate_ir::Guard {
                        name: "canToggle".into(),
                    });
                    atomic("off", vec![t])
                },
                atomic("on", vec![]),
            ],
        );

        let handle = system.spawn_machine("guarded", guarded);
        handle.send(serde_json::json!({"type": "TOGGLE"}));
        assert_eq!(
            handle.snapshot()["value"],
            "off",
            "registered guard returning false must block the transition in a natively spawned machine"
        );

        allowed.store(true, Ordering::SeqCst);
        handle.send(serde_json::json!({"type": "TOGGLE"}));
        assert_eq!(handle.snapshot()["value"], "on");
    }

    #[test]
    fn stop_parent_cascades_to_invoked_children() {
        let system = NativeActorSystem::new();
        system.register_machine("child", child_machine());
        let parent = system.spawn_machine("runner", parent_machine());

        parent.send(serde_json::json!({"type": "LOAD"}));
        assert_eq!(system.status("runner.child"), Some(ActorStatus::Running));

        system.stop("runner");
        assert_eq!(system.status("runner.child"), Some(ActorStatus::Stopped));
    }

    #[test]
    fn send_parent_without_parent_is_noop() {
        let child = machine(
            "lonely",
            "idle",
            vec![atomic(
                "idle",
                vec![action_trans(
                    "PING",
                    vec![Action {
                        name: "xstate.send_parent".into(),
                        args: vec![serde_json::json!({"type": "CHILD_DONE"})],
                    }],
                )],
            )],
        );

        let system = NativeActorSystem::new();
        let handle = system.spawn_machine("lonely", child);
        handle.send(serde_json::json!({"type": "PING"}));
        assert_eq!(
            handle.snapshot()["value"],
            "idle",
            "send_parent with no parent should not crash the actor"
        );
    }

    #[test]
    fn registry_action_composes_with_send_to_builtin() {
        let parent = machine(
            "parent",
            "idle",
            vec![atomic(
                "idle",
                vec![action_trans(
                    "TRIGGER",
                    vec![
                        Action {
                            name: "recordIt".into(),
                            args: vec![],
                        },
                        Action {
                            name: "xstate.send_to".into(),
                            args: vec![
                                serde_json::json!("child"),
                                serde_json::json!({"type": "POKE"}),
                            ],
                        },
                    ],
                )],
            )],
        );
        let child = machine(
            "child",
            "idle",
            vec![
                atomic("idle", vec![trans("POKE", "poked")]),
                atomic("poked", vec![]),
            ],
        );

        let system = NativeActorSystem::new();
        system.spawn_machine("child", child);
        system.set_registry_factory(|| {
            let mut actions = ActionRegistry::new();
            actions.register(ActionImpl::new("recordIt", |_, _, ctx| {
                ctx.as_object_mut()
                    .unwrap()
                    .insert("fired".into(), serde_json::json!(true));
            }));
            (GuardRegistry::new(), actions)
        });
        let parent = system.spawn_machine("parent", parent);

        parent.send(serde_json::json!({"type": "TRIGGER"}));
        assert_eq!(
            system.snapshot("parent").unwrap()["context"]["fired"],
            true,
            "factory-registered actions should dispatch by name"
        );
        assert_eq!(system.snapshot("child").unwrap()["value"], "poked");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xstate_core::actor_logic::ReducerLogic;

    #[test]
    fn spawn_and_send() {
        let system = NativeActorSystem::new();
        let logic = ReducerLogic::new(
            "counter",
            serde_json::json!({"count": 0}),
            |state, event| {
                let mut s = state.clone();
                if event["type"].as_str() == Some("INC") {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(c + 1));
                }
                (s, vec![])
            },
        );
        let handle = system.spawn("c1", Box::new(logic));
        assert_eq!(system.len(), 1);

        let snap = handle.snapshot();
        assert_eq!(snap["count"], 0);

        handle.send(serde_json::json!({"type": "INC"}));
        let snap = handle.snapshot();
        assert_eq!(snap["count"], 1);
    }

    #[test]
    fn system_dispatch_and_handle_dispatch_equivalent() {
        let system = NativeActorSystem::new();
        let logic = ReducerLogic::new(
            "counter",
            serde_json::json!({"count": 0}),
            |state, event| {
                let mut s = state.clone();
                if event["type"].as_str() == Some("INC") {
                    let c = s["count"].as_i64().unwrap_or(0);
                    s.as_object_mut()
                        .unwrap()
                        .insert("count".into(), serde_json::json!(c + 1));
                }
                (s, vec![])
            },
        );
        let handle = system.spawn("c1", Box::new(logic));

        system.send_to("c1", serde_json::json!({"type": "INC"}));
        assert_eq!(handle.snapshot()["count"], 1);

        handle.send(serde_json::json!({"type": "INC"}));
        assert_eq!(handle.snapshot()["count"], 2);
        assert_eq!(system.snapshot("c1").unwrap()["count"], 2);
    }

    #[test]
    fn stop_actor() {
        let system = NativeActorSystem::new();
        let logic = ReducerLogic::new("r", serde_json::json!({}), |s, _| (s.clone(), vec![]));
        let handle = system.spawn("a1", Box::new(logic));
        assert_eq!(handle.status(), ActorStatus::Running);

        handle.stop();
        assert_eq!(handle.status(), ActorStatus::Stopped);
    }

    #[test]
    fn unknown_actor_returns_defaults() {
        let system = NativeActorSystem::new();
        assert!(system.snapshot("nonexistent").is_none());
        assert!(system.status("nonexistent").is_none());
    }

    #[test]
    fn multiple_actors_independent() {
        let system = NativeActorSystem::new();
        let logic_a = ReducerLogic::new("a", serde_json::json!({"v": 1}), |state, event| {
            let mut s = state.clone();
            if event["type"].as_str() == Some("INC") {
                let v = s["v"].as_i64().unwrap_or(0);
                s.as_object_mut()
                    .unwrap()
                    .insert("v".into(), serde_json::json!(v + 1));
            }
            (s, vec![])
        });
        let logic_b = ReducerLogic::new("b", serde_json::json!({"v": 10}), |state, event| {
            let mut s = state.clone();
            if event["type"].as_str() == Some("INC") {
                let v = s["v"].as_i64().unwrap_or(0);
                s.as_object_mut()
                    .unwrap()
                    .insert("v".into(), serde_json::json!(v + 1));
            }
            (s, vec![])
        });

        let ha = system.spawn("a", Box::new(logic_a));
        let hb = system.spawn("b", Box::new(logic_b));

        ha.send(serde_json::json!({"type": "INC"}));
        ha.send(serde_json::json!({"type": "INC"}));

        assert_eq!(ha.snapshot()["v"], 3);
        assert_eq!(hb.snapshot()["v"], 10);
    }

    #[test]
    fn stopped_actor_ignores_events() {
        let system = NativeActorSystem::new();
        let logic = ReducerLogic::new("r", serde_json::json!({"v": 0}), |state, event| {
            let mut s = state.clone();
            if event["type"].as_str() == Some("INC") {
                let v = s["v"].as_i64().unwrap_or(0);
                s.as_object_mut()
                    .unwrap()
                    .insert("v".into(), serde_json::json!(v + 1));
            }
            (s, vec![])
        });
        let handle = system.spawn("r", Box::new(logic));
        handle.stop();
        handle.send(serde_json::json!({"type": "INC"}));
        assert_eq!(handle.snapshot()["v"], 0);
    }

    #[test]
    fn is_empty_len() {
        let system = NativeActorSystem::new();
        assert!(system.is_empty());
        assert_eq!(system.len(), 0);

        let logic = ReducerLogic::new("r", serde_json::json!({}), |s, _| (s.clone(), vec![]));
        system.spawn("a", Box::new(logic));
        assert!(!system.is_empty());
        assert_eq!(system.len(), 1);
    }

    #[test]
    fn machine_logic_guard_registry_blocks_and_fires_transition() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        use xstate_core::actions::ActionRegistry;
        use xstate_core::actor_logic::MachineLogic;
        use xstate_core::clock::MockClock;
        use xstate_core::guards::{GuardImpl, GuardRegistry};
        use xstate_core::interpreter::Interpreter;
        use xstate_core::xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

        let allowed = Arc::new(AtomicBool::new(false));
        let flag = allowed.clone();
        let mut guards = GuardRegistry::new();
        guards.register(GuardImpl::new("canToggle", move |_, _| {
            flag.load(Ordering::SeqCst)
        }));

        let machine = Machine {
            id: "guarded".into(),
            initial: "off".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                StateNode {
                    name: "off".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "TOGGLE".into(),
                        target: Some("on".into()),
                        guard: Some(xstate_core::xstate_ir::Guard {
                            name: "canToggle".into(),
                        }),
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
                StateNode {
                    name: "on".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
            ],
            setup: None,
        };

        let logic = MachineLogic::new_with_registries(
            Interpreter::new(machine),
            MockClock::new(0),
            guards,
            ActionRegistry::new(),
        );
        let system = NativeActorSystem::new();
        let handle = system.spawn("guarded", Box::new(logic));

        handle.send(serde_json::json!({"type": "TOGGLE"}));
        assert_eq!(handle.snapshot()["value"], "off");

        allowed.store(true, Ordering::SeqCst);
        handle.send(serde_json::json!({"type": "TOGGLE"}));
        assert_eq!(handle.snapshot()["value"], "on");
    }
}
