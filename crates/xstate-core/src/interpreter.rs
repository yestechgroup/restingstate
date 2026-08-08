use xstate_ir::{Machine, StateNode, StateType};

use crate::state_value::StateValue;

pub struct Interpreter {
    machine: Machine,
}

impl Interpreter {
    pub fn new(machine: Machine) -> Self {
        Interpreter { machine }
    }

    pub fn initial_state(&self) -> StateValue {
        let initial_name = &self.machine.initial;
        let node = self.find_root_state(initial_name).unwrap_or_else(|| {
            panic!(
                "Initial state '{}' not found in machine states",
                initial_name
            )
        });
        let value = self.resolve_initial_value(node);
        if node.node_type == StateType::Parallel {
            if let StateValue::Compound { child, .. } = value {
                return *child;
            }
        }
        value
    }

    pub fn initial_context(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for field in &self.machine.context.fields {
            map.insert(field.name.clone(), field.default_value.clone());
        }
        serde_json::Value::Object(map)
    }

    pub fn send(
        &self,
        state: &StateValue,
        event: &str,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value]),
    ) -> Option<StateValue> {
        match state {
            StateValue::Parallel { regions } => {
                self.send_parallel(regions, event, context, guards, actions)
            }
            StateValue::Compound { state: name, child } if self.contains_parallel(child) => {
                let new_child = self.send(child, event, context, guards, actions)?;
                Some(StateValue::Compound {
                    state: name.clone(),
                    child: Box::new(new_child),
                })
            }
            _ => {
                let path = self.find_full_path(state);
                self.send_along_path(&path, state, event, context, guards, actions)
            }
        }
    }

    fn contains_parallel(&self, state: &StateValue) -> bool {
        match state {
            StateValue::Parallel { .. } => true,
            StateValue::Compound { child, .. } => self.contains_parallel(child),
            _ => false,
        }
    }

    fn send_parallel(
        &self,
        regions: &[(String, StateValue)],
        event: &str,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value]),
    ) -> Option<StateValue> {
        let region_name = &regions.first()?.0;
        let region_path = Self::find_node_path_in(&self.machine.states, region_name)?;
        if region_path.len() < 2 {
            return None;
        }
        let parallel_node = region_path[region_path.len() - 2];
        let scope = &parallel_node.children;

        let mut changed = false;
        let mut new_regions = Vec::with_capacity(regions.len());

        for (region_name, region_value) in regions {
            let region_node = scope.iter().find(|n| &n.name == region_name);
            let region_scope = region_node.map(|n| &n.children[..]).unwrap_or(&[]);
            let transition_scope = region_scope.first().map(|_| region_scope).unwrap_or(scope);
            match self.send_scoped(
                region_value,
                region_scope,
                transition_scope,
                event,
                context,
                guards,
                actions,
            ) {
                Some(new_value) => {
                    changed = true;
                    new_regions.push((region_name.clone(), new_value));
                }
                None => {
                    new_regions.push((region_name.clone(), region_value.clone()));
                }
            }
        }

        if changed {
            Some(StateValue::Parallel {
                regions: new_regions,
            })
        } else {
            None
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn send_scoped(
        &self,
        state: &StateValue,
        search_scope: &[StateNode],
        transition_scope: &[StateNode],
        event: &str,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value]),
    ) -> Option<StateValue> {
        match state {
            StateValue::Atomic(name) => {
                let node = search_scope
                    .iter()
                    .find(|n| n.name == *name)
                    .or_else(|| transition_scope.iter().find(|n| n.name == *name))?;
                if node.node_type == StateType::Final {
                    return None;
                }
                for transition in &node.transitions {
                    if transition.event != event {
                        continue;
                    }
                    if let Some(ref guard) = transition.guard {
                        if !guards(&guard.name, context) {
                            continue;
                        }
                    }
                    if let Some(ref target_name) = transition.target {
                        let target = transition_scope.iter().find(|n| n.name == *target_name)?;
                        let target_value = self.resolve_region_value(target);
                        for action_name in &node.exit {
                            actions(action_name, &[]);
                        }
                        for action in &transition.actions {
                            actions(&action.name, &action.args);
                        }
                        for action_name in &target.entry {
                            actions(action_name, &[]);
                        }
                        return Some(target_value);
                    } else {
                        for action in &transition.actions {
                            actions(&action.name, &action.args);
                        }
                        return Some(state.clone());
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn find_full_path(&self, value: &StateValue) -> Vec<&StateNode> {
        match value {
            StateValue::Atomic(name) => {
                Self::find_node_path_in(&self.machine.states, name).unwrap_or_default()
            }
            StateValue::Compound { state, child } => {
                let mut path =
                    Self::find_node_path_in(&self.machine.states, state).unwrap_or_default();
                if let Some(parent) = path.last().copied() {
                    if let StateValue::Parallel { regions } = child.as_ref() {
                        if let Some((first_name, _)) = regions.first() {
                            let region_path = Self::find_node_path_in(&parent.children, first_name);
                            if let Some(rpath) = region_path {
                                path.extend(rpath);
                            }
                        }
                    } else {
                        let child_path = self.find_full_path_inner(child, &parent.children);
                        path.extend(child_path);
                    }
                }
                path
            }
            StateValue::Parallel { .. } => vec![],
        }
    }

    fn find_full_path_inner<'a>(
        &self,
        value: &StateValue,
        children: &'a [StateNode],
    ) -> Vec<&'a StateNode> {
        match value {
            StateValue::Atomic(name) => Self::find_node_path_in(children, name).unwrap_or_default(),
            StateValue::Compound { state, child } => {
                let mut path = Self::find_node_path_in(children, state).unwrap_or_default();
                if let Some(parent) = path.last().copied() {
                    let child_path = self.find_full_path_inner(child, &parent.children);
                    path.extend(child_path);
                }
                path
            }
            _ => vec![],
        }
    }

    fn send_along_path(
        &self,
        path: &[&StateNode],
        current_state: &StateValue,
        event: &str,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value]),
    ) -> Option<StateValue> {
        for node in path.iter().rev() {
            if node.node_type == StateType::Final {
                continue;
            }
            for transition in &node.transitions {
                if transition.event != event {
                    continue;
                }
                if let Some(ref guard) = transition.guard {
                    if !guards(&guard.name, context) {
                        continue;
                    }
                }

                if let Some(ref target_name) = transition.target {
                    let (target_path, target_value) = self.build_target(target_name)?;

                    let common_prefix = path
                        .iter()
                        .zip(target_path.iter())
                        .take_while(|(s, t)| s.name == t.name)
                        .count();

                    for n in path[common_prefix..].iter().rev() {
                        for action_name in &n.exit {
                            actions(action_name, &[]);
                        }
                    }

                    for action in &transition.actions {
                        actions(&action.name, &action.args);
                    }

                    for n in &target_path[common_prefix..] {
                        for action_name in &n.entry {
                            actions(action_name, &[]);
                        }
                    }

                    return Some(target_value);
                } else {
                    for action in &transition.actions {
                        actions(&action.name, &action.args);
                    }
                    return Some(current_state.clone());
                }
            }
        }

        None
    }

    fn build_target(&self, target_name: &str) -> Option<(Vec<&StateNode>, StateValue)> {
        let (path_to_named, leaf) = Self::find_node_by_name_in(&self.machine.states, target_name)?;

        let value = self.state_value_from_path(&path_to_named);

        let mut full_path: Vec<&StateNode> = path_to_named.clone();
        self.expand_initial_path(leaf, &mut full_path);

        Some((full_path, value))
    }

    fn state_value_from_path(&self, path: &[&StateNode]) -> StateValue {
        let leaf = path.last().expect("path should not be empty");
        let mut value = self.resolve_initial_value(leaf);
        for ancestor in path[..path.len().saturating_sub(1)].iter().rev() {
            value = StateValue::Compound {
                state: ancestor.name.clone(),
                child: Box::new(value),
            };
        }
        value
    }

    fn expand_initial_path<'a>(&self, node: &'a StateNode, path: &mut Vec<&'a StateNode>) {
        if node.node_type == StateType::Compound {
            if let Some(initial_name) = &node.initial {
                if let Some(child) = node.children.iter().find(|c| &c.name == initial_name) {
                    path.push(child);
                    self.expand_initial_path(child, path);
                }
            }
        }
    }

    fn resolve_initial_value(&self, node: &StateNode) -> StateValue {
        match node.node_type {
            StateType::Compound => {
                if let Some(initial_name) = &node.initial {
                    let child = node
                        .children
                        .iter()
                        .find(|c| &c.name == initial_name)
                        .unwrap_or_else(|| {
                            panic!(
                                "Initial child '{}' not found in compound state '{}'",
                                initial_name, node.name
                            )
                        });
                    StateValue::Compound {
                        state: node.name.clone(),
                        child: Box::new(self.resolve_initial_value(child)),
                    }
                } else {
                    StateValue::Atomic(node.name.clone())
                }
            }
            StateType::Parallel => {
                let regions: Vec<_> = node
                    .children
                    .iter()
                    .map(|c| (c.name.clone(), self.resolve_region_value(c)))
                    .collect();
                StateValue::Compound {
                    state: node.name.clone(),
                    child: Box::new(StateValue::Parallel { regions }),
                }
            }
            _ => StateValue::Atomic(node.name.clone()),
        }
    }

    fn resolve_region_value(&self, node: &StateNode) -> StateValue {
        match node.node_type {
            StateType::Compound => {
                if let Some(initial_name) = &node.initial {
                    let child = match node.children.iter().find(|c| &c.name == initial_name) {
                        Some(c) => c,
                        None => return StateValue::Atomic(node.name.clone()),
                    };
                    self.resolve_initial_value(child)
                } else {
                    StateValue::Atomic(node.name.clone())
                }
            }
            _ => self.resolve_initial_value(node),
        }
    }

    fn find_root_state(&self, name: &str) -> Option<&StateNode> {
        Self::find_in_children(&self.machine.states, name)
    }

    fn find_node_by_name_in<'a>(
        states: &'a [StateNode],
        name: &str,
    ) -> Option<(Vec<&'a StateNode>, &'a StateNode)> {
        for state in states {
            if state.name == name {
                return Some((vec![state], state));
            }
            if let Some((mut path, found)) = Self::find_node_by_name_in(&state.children, name) {
                let mut full_path = vec![state];
                full_path.append(&mut path);
                return Some((full_path, found));
            }
        }
        None
    }

    fn find_node_path_in<'a>(states: &'a [StateNode], name: &str) -> Option<Vec<&'a StateNode>> {
        for state in states {
            if state.name == name {
                return Some(vec![state]);
            }
            if let Some(mut path) = Self::find_node_path_in(&state.children, name) {
                let mut full_path = vec![state];
                full_path.append(&mut path);
                return Some(full_path);
            }
        }
        None
    }

    fn find_in_children<'a>(states: &'a [StateNode], name: &str) -> Option<&'a StateNode> {
        for state in states {
            if state.name == name {
                return Some(state);
            }
            if let Some(found) = Self::find_in_children(&state.children, name) {
                return Some(found);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xstate_ir::{
        Action, ContextField, ContextSchema, Guard, StateNode as IrStateNode, StateType, Transition,
    };

    fn toggle_machine() -> Machine {
        Machine {
            id: "toggle".into(),
            initial: "inactive".into(),
            context: ContextSchema {
                fields: vec![ContextField {
                    name: "count".into(),
                    type_name: "number".into(),
                    default_value: serde_json::json!(0),
                }],
            },
            states: vec![
                IrStateNode {
                    name: "inactive".into(),
                    description: None,
                    initial: None,
                    entry: vec!["entered_inactive".into()],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "toggle".into(),
                        target: Some("active".into()),
                        guard: None,
                        actions: vec![Action {
                            name: "increment".into(),
                            args: vec![],
                        }],
                        description: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
                IrStateNode {
                    name: "active".into(),
                    description: None,
                    initial: None,
                    entry: vec!["entered_active".into()],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "toggle".into(),
                        target: Some("inactive".into()),
                        guard: None,
                        actions: vec![Action {
                            name: "increment".into(),
                            args: vec![],
                        }],
                        description: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
            ],
        }
    }

    #[test]
    fn initial_state_atomic() {
        let interpreter = Interpreter::new(toggle_machine());
        let state = interpreter.initial_state();
        assert_eq!(state, StateValue::Atomic("inactive".into()));
    }

    #[test]
    fn initial_context() {
        let interpreter = Interpreter::new(toggle_machine());
        let ctx = interpreter.initial_context();
        assert_eq!(ctx, serde_json::json!({"count": 0}));
    }

    #[test]
    fn toggle_transition() {
        let interpreter = Interpreter::new(toggle_machine());
        let state = StateValue::Atomic("inactive".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value]| {
            actions_log.push(name.to_string());
        };
        let guards = |_name: &str, _ctx: &serde_json::Value| true;

        let new_state = interpreter
            .send(&state, "toggle", &mut context, &guards, &mut actions)
            .unwrap();
        assert_eq!(new_state, StateValue::Atomic("active".into()));
        assert!(actions_log.contains(&"increment".to_string()));
    }

    #[test]
    fn no_matching_event_returns_none() {
        let interpreter = Interpreter::new(toggle_machine());
        let state = StateValue::Atomic("inactive".into());
        let mut context = interpreter.initial_context();

        let mut actions = |_: &str, _: &[serde_json::Value]| {};
        let guards = |_: &str, _: &serde_json::Value| true;

        let result = interpreter.send(&state, "unknown", &mut context, &guards, &mut actions);
        assert!(result.is_none());
    }

    #[test]
    fn guard_false_skips_transition() {
        let interpreter = Interpreter::new(payment_machine());
        let state = StateValue::Atomic("idle".into());
        let mut context = interpreter.initial_context();

        let mut actions = |_: &str, _: &[serde_json::Value]| {};
        let guards = |_: &str, _: &serde_json::Value| false;

        let result = interpreter.send(&state, "PAY", &mut context, &guards, &mut actions);
        assert!(result.is_none());
    }

    fn payment_machine() -> Machine {
        Machine {
            id: "paymentMachine".into(),
            initial: "idle".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "idle".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec!["exit_idle".into()],
                    transitions: vec![Transition {
                        event: "PAY".into(),
                        target: Some("processing".into()),
                        guard: Some(Guard {
                            name: "isAmountValid".into(),
                        }),
                        actions: vec![Action {
                            name: "startPayment".into(),
                            args: vec![],
                        }],
                        description: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
                IrStateNode {
                    name: "processing".into(),
                    description: None,
                    initial: Some("auth".into()),
                    entry: vec!["enter_processing".into()],
                    exit: vec!["exit_processing".into()],
                    transitions: vec![],
                    children: vec![
                        IrStateNode {
                            name: "auth".into(),
                            description: None,
                            initial: None,
                            entry: vec!["enter_auth".into()],
                            exit: vec!["exit_auth".into()],
                            transitions: vec![Transition {
                                event: "AUTH_OK".into(),
                                target: Some("capture".into()),
                                guard: None,
                                actions: vec![Action {
                                    name: "authSuccess".into(),
                                    args: vec![],
                                }],
                                description: None,
                            }],
                            children: vec![],
                            node_type: StateType::Atomic,
                            tags: vec![],
                        },
                        IrStateNode {
                            name: "capture".into(),
                            description: None,
                            initial: None,
                            entry: vec!["enter_capture".into()],
                            exit: vec![],
                            transitions: vec![Transition {
                                event: "CAPTURE_OK".into(),
                                target: Some("done".into()),
                                guard: None,
                                actions: vec![],
                                description: None,
                            }],
                            children: vec![],
                            node_type: StateType::Atomic,
                            tags: vec![],
                        },
                    ],
                    node_type: StateType::Compound,
                    tags: vec![],
                },
                IrStateNode {
                    name: "done".into(),
                    description: None,
                    initial: None,
                    entry: vec!["enter_done".into()],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Final,
                    tags: vec![],
                },
            ],
        }
    }

    #[test]
    fn compound_initial_state() {
        let interpreter = Interpreter::new(payment_machine());
        let state = interpreter.initial_state();
        assert_eq!(state, StateValue::Atomic("idle".into()));
    }

    #[test]
    fn compound_enter_initial_child() {
        let interpreter = Interpreter::new(payment_machine());
        let state = StateValue::Atomic("idle".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value]| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(&state, "PAY", &mut context, &guards, &mut actions)
            .unwrap();

        assert_eq!(
            new_state,
            StateValue::Compound {
                state: "processing".into(),
                child: Box::new(StateValue::Atomic("auth".into())),
            }
        );

        assert!(actions_log.contains(&"exit_idle".to_string()));
        assert!(actions_log.contains(&"startPayment".to_string()));
        assert!(actions_log.contains(&"enter_processing".to_string()));
        assert!(actions_log.contains(&"enter_auth".to_string()));
    }

    #[test]
    fn compound_child_transition() {
        let interpreter = Interpreter::new(payment_machine());
        let processing_auth = StateValue::Compound {
            state: "processing".into(),
            child: Box::new(StateValue::Atomic("auth".into())),
        };
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value]| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &processing_auth,
                "AUTH_OK",
                &mut context,
                &guards,
                &mut actions,
            )
            .unwrap();

        assert_eq!(
            new_state,
            StateValue::Compound {
                state: "processing".into(),
                child: Box::new(StateValue::Atomic("capture".into())),
            }
        );

        assert!(actions_log.contains(&"exit_auth".to_string()));
        assert!(actions_log.contains(&"authSuccess".to_string()));
        assert!(actions_log.contains(&"enter_capture".to_string()));
        assert!(!actions_log.contains(&"exit_processing".to_string()));
        assert!(!actions_log.contains(&"enter_processing".to_string()));
    }

    #[test]
    fn compound_to_root_transition() {
        let interpreter = Interpreter::new(payment_machine());
        let processing_capture = StateValue::Compound {
            state: "processing".into(),
            child: Box::new(StateValue::Atomic("capture".into())),
        };
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value]| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &processing_capture,
                "CAPTURE_OK",
                &mut context,
                &guards,
                &mut actions,
            )
            .unwrap();

        assert_eq!(new_state, StateValue::Atomic("done".into()));
        assert!(actions_log.contains(&"exit_processing".to_string()));
        assert!(actions_log.contains(&"enter_done".to_string()));
    }

    #[test]
    fn final_state_no_transition() {
        let interpreter = Interpreter::new(payment_machine());
        let done_state = StateValue::Atomic("done".into());
        let mut context = interpreter.initial_context();

        let mut actions = |_: &str, _: &[serde_json::Value]| {};
        let guards = |_: &str, _: &serde_json::Value| true;

        let result = interpreter.send(
            &done_state,
            "CAPTURE_OK",
            &mut context,
            &guards,
            &mut actions,
        );
        assert!(result.is_none());
    }

    #[test]
    fn no_target_transition_executes_actions() {
        let machine = Machine {
            id: "log".into(),
            initial: "idle".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![IrStateNode {
                name: "idle".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![Transition {
                    event: "LOG".into(),
                    target: None,
                    guard: None,
                    actions: vec![Action {
                        name: "logSomething".into(),
                        args: vec![],
                    }],
                    description: None,
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
            }],
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("idle".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value]| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value| true;

        let result = interpreter.send(&state, "LOG", &mut context, &guards, &mut actions);
        assert_eq!(result, Some(StateValue::Atomic("idle".into())));
        assert!(actions_log.contains(&"logSomething".to_string()));
    }

    #[test]
    fn parallel_initial_state() {
        let machine = parallel_dashboard_machine();
        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        match &state {
            StateValue::Parallel { regions } => {
                assert_eq!(regions.len(), 4);
            }
            _ => panic!("Expected parallel state, got {:?}", state),
        }
    }

    #[test]
    fn parallel_event_dispatches_to_all_regions() {
        let machine = parallel_dashboard_machine();
        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        let mut context = interpreter.initial_context();

        let mut actions = |_: &str, _: &[serde_json::Value]| {};
        let guards = |_: &str, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(&state, "DONE", &mut context, &guards, &mut actions)
            .unwrap();

        match &new_state {
            StateValue::Parallel { regions } => {
                let fetching = regions
                    .iter()
                    .find(|(n, _)| n == "fetching")
                    .unwrap()
                    .1
                    .clone();
                let rendering = regions
                    .iter()
                    .find(|(n, _)| n == "rendering")
                    .unwrap()
                    .1
                    .clone();
                assert_eq!(fetching, StateValue::Atomic("loaded".into()));
                assert_eq!(rendering, StateValue::Atomic("rendered".into()));
            }
            _ => panic!("Expected parallel state, got {:?}", new_state),
        }
    }

    fn parallel_dashboard_machine() -> Machine {
        Machine {
            id: "dashboard".into(),
            initial: "loading".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![IrStateNode {
                name: "loading".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![
                    IrStateNode {
                        name: "fetching".into(),
                        description: None,
                        initial: None,
                        entry: vec![],
                        exit: vec![],
                        transitions: vec![Transition {
                            event: "DONE".into(),
                            target: Some("loaded".into()),
                            guard: None,
                            actions: vec![],
                            description: None,
                        }],
                        children: vec![],
                        node_type: StateType::Atomic,
                        tags: vec![],
                    },
                    IrStateNode {
                        name: "rendering".into(),
                        description: None,
                        initial: None,
                        entry: vec![],
                        exit: vec![],
                        transitions: vec![Transition {
                            event: "DONE".into(),
                            target: Some("rendered".into()),
                            guard: None,
                            actions: vec![],
                            description: None,
                        }],
                        children: vec![],
                        node_type: StateType::Atomic,
                        tags: vec![],
                    },
                    IrStateNode {
                        name: "loaded".into(),
                        description: None,
                        initial: None,
                        entry: vec![],
                        exit: vec![],
                        transitions: vec![],
                        children: vec![],
                        node_type: StateType::Atomic,
                        tags: vec![],
                    },
                    IrStateNode {
                        name: "rendered".into(),
                        description: None,
                        initial: None,
                        entry: vec![],
                        exit: vec![],
                        transitions: vec![],
                        children: vec![],
                        node_type: StateType::Atomic,
                        tags: vec![],
                    },
                ],
                node_type: StateType::Parallel,
                tags: vec![],
            }],
        }
    }

    #[test]
    fn machine_with_deeply_nested_initial() {
        let machine = Machine {
            id: "deep".into(),
            initial: "a".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![IrStateNode {
                name: "a".into(),
                description: None,
                initial: Some("b".into()),
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![IrStateNode {
                    name: "b".into(),
                    description: None,
                    initial: Some("c".into()),
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![IrStateNode {
                        name: "c".into(),
                        description: None,
                        initial: None,
                        entry: vec![],
                        exit: vec![],
                        transitions: vec![],
                        children: vec![],
                        node_type: StateType::Atomic,
                        tags: vec![],
                    }],
                    node_type: StateType::Compound,
                    tags: vec![],
                }],
                node_type: StateType::Compound,
                tags: vec![],
            }],
        };

        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        assert_eq!(
            state,
            StateValue::Compound {
                state: "a".into(),
                child: Box::new(StateValue::Compound {
                    state: "b".into(),
                    child: Box::new(StateValue::Atomic("c".into())),
                }),
            }
        );
    }

    #[test]
    fn actions_receive_args() {
        let machine = Machine {
            id: "args".into(),
            initial: "start".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "start".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "GO".into(),
                        target: Some("end".into()),
                        guard: None,
                        actions: vec![Action {
                            name: "assign".into(),
                            args: vec![serde_json::json!("key"), serde_json::json!(42)],
                        }],
                        description: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
                IrStateNode {
                    name: "end".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
            ],
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("start".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<(String, Vec<serde_json::Value>)> = Vec::new();

        let mut actions = |name: &str, args: &[serde_json::Value]| {
            actions_log.push((name.to_string(), args.to_vec()));
        };
        let guards = |_: &str, _: &serde_json::Value| true;

        interpreter
            .send(&state, "GO", &mut context, &guards, &mut actions)
            .unwrap();

        assert_eq!(actions_log.len(), 1);
        assert_eq!(actions_log[0].0, "assign");
        assert_eq!(actions_log[0].1.len(), 2);
    }

    #[test]
    fn exit_actions_run_on_compound_transition() {
        let machine = Machine {
            id: "compound_exit".into(),
            initial: "outer".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "outer".into(),
                    description: None,
                    initial: Some("inner".into()),
                    entry: vec![],
                    exit: vec!["exit_outer".into()],
                    transitions: vec![Transition {
                        event: "NEXT".into(),
                        target: Some("other".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                    }],
                    children: vec![IrStateNode {
                        name: "inner".into(),
                        description: None,
                        initial: None,
                        entry: vec![],
                        exit: vec!["exit_inner".into()],
                        transitions: vec![],
                        children: vec![],
                        node_type: StateType::Atomic,
                        tags: vec![],
                    }],
                    node_type: StateType::Compound,
                    tags: vec![],
                },
                IrStateNode {
                    name: "other".into(),
                    description: None,
                    initial: None,
                    entry: vec!["enter_other".into()],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
            ],
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Compound {
            state: "outer".into(),
            child: Box::new(StateValue::Atomic("inner".into())),
        };
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value]| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value| true;

        interpreter
            .send(&state, "NEXT", &mut context, &guards, &mut actions)
            .unwrap();

        assert!(actions_log.contains(&"exit_inner".to_string()));
        assert!(actions_log.contains(&"exit_outer".to_string()));
        assert!(actions_log.contains(&"enter_other".to_string()));
    }

    #[test]
    fn simple_atomic_entry_exit() {
        let machine = Machine {
            id: "simple".into(),
            initial: "a".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "a".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec!["exit_a".into()],
                    transitions: vec![Transition {
                        event: "NEXT".into(),
                        target: Some("b".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
                IrStateNode {
                    name: "b".into(),
                    description: None,
                    initial: None,
                    entry: vec!["enter_b".into()],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
            ],
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("a".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value]| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value| true;

        interpreter
            .send(&state, "NEXT", &mut context, &guards, &mut actions)
            .unwrap();

        assert!(actions_log.contains(&"exit_a".to_string()));
        assert!(actions_log.contains(&"enter_b".to_string()));
    }

    #[test]
    fn entry_actions_on_compound_initial_not_triggered_during_send() {
        let machine = Machine {
            id: "entry_order".into(),
            initial: "a".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "a".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec!["exit_a".into()],
                    transitions: vec![Transition {
                        event: "NEXT".into(),
                        target: Some("b".into()),
                        guard: None,
                        actions: vec![Action {
                            name: "trans_action".into(),
                            args: vec![],
                        }],
                        description: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
                IrStateNode {
                    name: "b".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec!["exit_b".into()],
                    transitions: vec![Transition {
                        event: "NEXT".into(),
                        target: Some("a".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
            ],
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("a".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value]| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value| true;

        interpreter
            .send(&state, "NEXT", &mut context, &guards, &mut actions)
            .unwrap();

        let exit_idx = actions_log.iter().position(|a| a == "exit_a").unwrap();
        let trans_idx = actions_log
            .iter()
            .position(|a| a == "trans_action")
            .unwrap();
        assert!(
            exit_idx < trans_idx,
            "exit_a must fire before trans_action, but exit_a at {} and trans_action at {}",
            exit_idx,
            trans_idx
        );
    }
}
