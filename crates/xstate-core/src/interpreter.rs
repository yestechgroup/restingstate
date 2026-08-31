use std::cell::RefCell;
use std::collections::HashMap;
use xstate_ir::{Action, Machine, StateNode, StateType};

use crate::actions::ActionRegistry;
use crate::clock::Clock;
use crate::guards::GuardRegistry;
use crate::state_value::StateValue;

fn run_action(
    action: &Action,
    event_payload: &serde_json::Value,
    context: &mut serde_json::Value,
    actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
) {
    actions(&action.name, &action.args, event_payload);
    if action.name == "assign" {
        let chunks = action.args.chunks(2);
        for chunk in chunks {
            if chunk.len() == 2
                && let Some(key_str) = chunk[0].as_str()
                && let Some(obj) = context.as_object_mut()
            {
                obj.insert(key_str.to_string(), chunk[1].clone());
            }
        }
    }
}

fn run_action_by_name(
    action_name: &str,
    event_payload: &serde_json::Value,
    _context: &mut serde_json::Value,
    actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
) {
    actions(action_name, &[], event_payload);
}

pub struct Interpreter {
    machine: Machine,
    history: RefCell<HashMap<String, StateValue>>,
    entry_times: RefCell<HashMap<String, u64>>,
}

impl Interpreter {
    pub fn new(machine: Machine) -> Self {
        let interpreter = Interpreter {
            machine,
            history: RefCell::new(HashMap::new()),
            entry_times: RefCell::new(HashMap::new()),
        };
        interpreter.record_initial_history();
        interpreter.record_initial_entry_times();
        interpreter
    }

    fn record_initial_entry_times(&self) {
        let initial_name = self.machine.initial.clone();
        if let Some(node) = self.find_root_state(&initial_name) {
            self.record_entry_time_for_node(node);
        }
    }

    fn record_entry_time_for_node(&self, node: &StateNode) {
        self.entry_times.borrow_mut().insert(node.name.clone(), 0);
        match &node.node_type {
            StateType::Compound => {
                if let Some(initial_name) = &node.initial
                    && let Some(child) = node.children.iter().find(|c| &c.name == initial_name)
                {
                    self.record_entry_time_for_node(child);
                }
            }
            StateType::Parallel => {
                for child in &node.children {
                    self.record_entry_time_for_node(child);
                }
            }
            _ => {}
        }
    }

    fn record_initial_history(&self) {
        let initial_name = self.machine.initial.clone();
        if let Some(node) = self.find_root_state(&initial_name) {
            self.record_entry_history(node);
        }
    }

    fn record_entry_history(&self, node: &StateNode) {
        let has_history = node
            .children
            .iter()
            .any(|c| c.node_type == StateType::History || c.node_type == StateType::DeepHistory);
        if !has_history {
            return;
        }
        if let Some(initial_name) = &node.initial
            && let Some(child) = node.children.iter().find(|c| &c.name == initial_name)
        {
            let value = self.resolve_initial_value(child);
            self.history.borrow_mut().insert(node.name.clone(), value);
            self.record_entry_history(child);
        }
    }

    pub fn machine(&self) -> &Machine {
        &self.machine
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
        if node.node_type == StateType::Parallel
            && let StateValue::Compound { child, .. } = value
        {
            return *child;
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

    pub fn restore(
        &self,
        state_value: &StateValue,
        context: serde_json::Value,
    ) -> (StateValue, serde_json::Value) {
        self.validate_state_value(state_value, &self.machine.states);
        (state_value.clone(), context)
    }

    pub fn snapshot(
        &self,
        state_value: &StateValue,
        context: &serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "value": serde_json::to_value(state_value).unwrap(),
            "context": context,
        })
    }

    fn validate_state_value(&self, value: &StateValue, scope: &[StateNode]) {
        match value {
            StateValue::Atomic(name) => {
                if !scope.iter().any(|s| s.name == *name) {
                    panic!("State '{}' not found in machine", name);
                }
            }
            StateValue::Compound { state, child } => {
                let node = scope
                    .iter()
                    .find(|s| s.name == *state)
                    .unwrap_or_else(|| panic!("State '{}' not found in machine", state));
                self.validate_state_value(child, &node.children);
            }
            StateValue::Parallel { regions } => {
                for (region_name, region_value) in regions {
                    let node = scope
                        .iter()
                        .find(|s| s.name == *region_name)
                        .unwrap_or_else(|| panic!("Region '{}' not found in machine", region_name));
                    self.validate_state_value(region_value, &node.children);
                }
            }
        }
    }

    pub fn send(
        &self,
        state: &StateValue,
        event: &serde_json::Value,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
        clock: &dyn Clock,
    ) -> Option<StateValue> {
        let event_type = event["type"].as_str().unwrap_or("");
        let mut current = state.clone();
        let result = match &current {
            StateValue::Parallel { regions } => {
                self.send_parallel(regions, event_type, event, context, guards, actions, clock)
            }
            StateValue::Compound { state: name, child } if self.contains_parallel(child) => {
                let new_child = self.send(child, event, context, guards, actions, clock)?;
                if self.contains_parallel(&new_child) {
                    // The parallel (or a region of it) handled the event
                    // internally: preserve the compound wrapper.
                    Some(StateValue::Compound {
                        state: name.clone(),
                        child: Box::new(new_child),
                    })
                } else {
                    // The new state no longer contains the parallel: a
                    // transition on the compound node itself (or a history
                    // target) exited it, and the value is already complete.
                    Some(new_child)
                }
            }
            _ => {
                let path = self.find_full_path(&current);
                self.send_along_path(
                    &path, &current, event_type, event, context, guards, actions, clock,
                )
            }
        };

        let mut changed = false;
        if let Some(ref next_state) = result {
            current = next_state.clone();
            changed = true;
            for _ in 0..10 {
                match self.resolve_always(&current, context, guards, actions, clock) {
                    Some(always_state) => current = always_state,
                    None => break,
                }
            }
            for _ in 0..10 {
                match self.resolve_after(&current, context, guards, actions, clock) {
                    Some(after_state) => current = after_state,
                    None => break,
                }
            }
            for _ in 0..10 {
                match self.check_completion(&current, context, guards, actions, clock) {
                    Some(completed_state) => {
                        current = completed_state;
                        for _ in 0..10 {
                            match self.resolve_always(&current, context, guards, actions, clock) {
                                Some(always_state) => current = always_state,
                                None => break,
                            }
                        }
                    }
                    None => break,
                }
            }
        } else {
            for _ in 0..10 {
                match self.resolve_after(&current, context, guards, actions, clock) {
                    Some(after_state) => {
                        current = after_state;
                        changed = true;
                        for _ in 0..10 {
                            match self.resolve_always(&current, context, guards, actions, clock) {
                                Some(always_state) => current = always_state,
                                None => break,
                            }
                        }
                    }
                    None => break,
                }
            }
            for _ in 0..10 {
                match self.check_completion(&current, context, guards, actions, clock) {
                    Some(completed_state) => {
                        current = completed_state;
                        changed = true;
                        for _ in 0..10 {
                            match self.resolve_always(&current, context, guards, actions, clock) {
                                Some(always_state) => current = always_state,
                                None => break,
                            }
                        }
                    }
                    None => break,
                }
            }
        }
        if changed { Some(current) } else { None }
    }

    /// Like `send`, but uses `GuardRegistry` / `ActionRegistry` instead of closures.
    pub fn send_with_registry(
        &self,
        state: &StateValue,
        event: &serde_json::Value,
        context: &mut serde_json::Value,
        guards: &GuardRegistry,
        actions: &ActionRegistry,
        clock: &dyn Clock,
    ) -> Option<StateValue> {
        // SAFETY: We take a raw pointer to `context` so the actions closure can
        // access it without capturing `context` (which is already borrowed by
        // `send`). The closures are called sequentially by `send` (never
        // re-entrantly), so there is no data race.
        let raw = context as *mut serde_json::Value;
        self.send(
            state,
            event,
            context,
            &|name, ctx, evt| guards.evaluate(name, ctx, evt),
            &mut |name, args, evt| {
                // SAFETY: `raw` points to the same `context` that `send` borrows.
                unsafe { actions.dispatch(name, args, evt, &mut *raw) };
            },
            clock,
        )
    }

    fn resolve_always(
        &self,
        state: &StateValue,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
        clock: &dyn Clock,
    ) -> Option<StateValue> {
        let path = self.find_full_path(state);
        if path.is_empty() {
            return None;
        }
        let empty_event = serde_json::json!({"type": ""});
        self.send_along_path(
            &path,
            state,
            "",
            &empty_event,
            context,
            guards,
            actions,
            clock,
        )
    }

    fn resolve_after(
        &self,
        state: &StateValue,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
        clock: &dyn Clock,
    ) -> Option<StateValue> {
        let path = self.find_full_path(state);
        if path.is_empty() {
            return None;
        }
        let now = clock.now_ms();

        for node in path.iter().rev() {
            if node.node_type == StateType::Final {
                continue;
            }
            for transition in &node.transitions {
                let Some(delay_ms) = transition.delay_ms else {
                    continue;
                };
                let entered_at = self
                    .entry_times
                    .borrow()
                    .get(&node.name)
                    .copied()
                    .unwrap_or(0);
                if now < entered_at + delay_ms {
                    continue;
                }
                if let Some(ref guard) = transition.guard
                    && !guards(&guard.name, context, &serde_json::Value::Null)
                {
                    continue;
                }
                if let Some(ref target_name) = transition.target {
                    let (target_path, target_value) = self.build_target(target_name)?;
                    let common_prefix = path
                        .iter()
                        .zip(target_path.iter())
                        .take_while(|(s, t)| s.name == t.name)
                        .count();

                    for i in common_prefix..path.len() {
                        if i > 0 {
                            let parent = path[i - 1];
                            let has_history = parent.children.iter().any(|c| {
                                c.node_type == StateType::History
                                    || c.node_type == StateType::DeepHistory
                            });
                            if has_history {
                                let child_value = self.resolve_initial_value(path[i]);
                                self.history
                                    .borrow_mut()
                                    .insert(parent.name.clone(), child_value);
                            }
                        }
                    }
                    for n in path[common_prefix..].iter().rev() {
                        for action_name in &n.exit {
                            run_action_by_name(
                                action_name,
                                &serde_json::Value::Null,
                                context,
                                actions,
                            );
                        }
                    }
                    for action in &transition.actions {
                        run_action(action, &serde_json::Value::Null, context, actions);
                    }
                    for n in &target_path[common_prefix..] {
                        for action_name in &n.entry {
                            run_action_by_name(
                                action_name,
                                &serde_json::Value::Null,
                                context,
                                actions,
                            );
                        }
                        self.entry_times.borrow_mut().insert(n.name.clone(), now);
                    }
                    return Some(target_value);
                }
            }
        }
        None
    }

    fn contains_parallel(&self, state: &StateValue) -> bool {
        match state {
            StateValue::Parallel { .. } => true,
            StateValue::Compound { child, .. } => self.contains_parallel(child),
            _ => false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn check_completion(
        &self,
        state: &StateValue,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
        clock: &dyn Clock,
    ) -> Option<StateValue> {
        let path = self.find_full_path(state);
        if path.is_empty() {
            return None;
        }
        for i in (0..path.len().saturating_sub(1)).rev() {
            let parent = path[i];
            if parent.on_done.is_empty() {
                continue;
            }

            let is_complete = match parent.node_type {
                StateType::Compound => path
                    .get(i + 1)
                    .is_some_and(|child| child.node_type == StateType::Final),
                StateType::Parallel => {
                    if let Some(parallel_value) = self.extract_state_value_at(state, i, &path) {
                        self.is_parallel_value_final(&parallel_value)
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if !is_complete {
                continue;
            }

            for transition in &parent.on_done {
                if let Some(ref guard) = transition.guard
                    && !guards(&guard.name, context, &serde_json::Value::Null)
                {
                    continue;
                }

                if !transition.targets.is_empty() {
                    if let Some((target_path, target_value)) =
                        self.build_multi_target(&transition.targets)
                    {
                        let common_prefix = path
                            .iter()
                            .zip(target_path.iter())
                            .take_while(|(s, t)| s.name == t.name)
                            .count();

                        for n in path[common_prefix..].iter().rev() {
                            for action_name in &n.exit {
                                run_action_by_name(
                                    action_name,
                                    &serde_json::Value::Null,
                                    context,
                                    actions,
                                );
                            }
                        }

                        let now = clock.now_ms();
                        for n in &target_path[common_prefix..] {
                            self.entry_times.borrow_mut().insert(n.name.clone(), now);
                        }

                        for action in &transition.actions {
                            run_action(action, &serde_json::Value::Null, context, actions);
                        }

                        for n in &target_path[common_prefix..] {
                            for action_name in &n.entry {
                                run_action_by_name(
                                    action_name,
                                    &serde_json::Value::Null,
                                    context,
                                    actions,
                                );
                            }
                        }

                        return Some(target_value);
                    }
                } else if let Some(ref target_name) = transition.target
                    && let Some((target_path, target_value)) = self.build_target(target_name)
                {
                    let common_prefix = path
                        .iter()
                        .zip(target_path.iter())
                        .take_while(|(s, t)| s.name == t.name)
                        .count();

                    for n in path[common_prefix..].iter().rev() {
                        for action_name in &n.exit {
                            run_action_by_name(
                                action_name,
                                &serde_json::Value::Null,
                                context,
                                actions,
                            );
                        }
                    }

                    let now = clock.now_ms();
                    for n in &target_path[common_prefix..] {
                        self.entry_times.borrow_mut().insert(n.name.clone(), now);
                    }

                    for action in &transition.actions {
                        run_action(action, &serde_json::Value::Null, context, actions);
                    }

                    for n in &target_path[common_prefix..] {
                        for action_name in &n.entry {
                            run_action_by_name(
                                action_name,
                                &serde_json::Value::Null,
                                context,
                                actions,
                            );
                        }
                    }

                    return Some(target_value);
                }
            }
        }
        None
    }

    fn extract_state_value_at(
        &self,
        state: &StateValue,
        target_index: usize,
        _path: &[&StateNode],
    ) -> Option<StateValue> {
        let mut current = state.clone();
        for i in 0..=target_index {
            match &current {
                StateValue::Compound { child, .. } => {
                    if i == target_index {
                        return Some(*child.clone());
                    }
                    current = *child.clone();
                }
                StateValue::Parallel { regions } => {
                    return Some(StateValue::Parallel {
                        regions: regions.clone(),
                    });
                }
                _ => return None,
            }
        }
        None
    }

    fn is_parallel_value_final(&self, value: &StateValue) -> bool {
        match value {
            StateValue::Parallel { regions } => {
                if regions.is_empty() {
                    return false;
                }
                regions.iter().all(|(region_name, region_value)| {
                    self.is_region_value_final(region_name, region_value)
                })
            }
            _ => false,
        }
    }

    fn is_region_value_final(&self, region_name: &str, value: &StateValue) -> bool {
        let region_node = Self::find_node_path_in(&self.machine.states, region_name)
            .and_then(|p| p.last().copied());
        if let Some(node) = region_node {
            self.check_child_final(value, node)
        } else {
            false
        }
    }

    fn check_child_final(&self, value: &StateValue, parent: &StateNode) -> bool {
        match value {
            StateValue::Atomic(name) => parent
                .children
                .iter()
                .any(|c| c.name == *name && c.node_type == StateType::Final),
            StateValue::Compound { state, child } => {
                if let Some(node) = parent.children.iter().find(|c| c.name == *state) {
                    self.check_child_final(child, node)
                } else {
                    false
                }
            }
            StateValue::Parallel { regions } => regions.iter().all(|(name, child_value)| {
                if let Some(node) = parent.children.iter().find(|c| c.name == *name) {
                    self.check_child_final(child_value, node)
                } else {
                    false
                }
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn send_parallel(
        &self,
        regions: &[(String, StateValue)],
        event: &str,
        event_payload: &serde_json::Value,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
        clock: &dyn Clock,
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
                event_payload,
                context,
                guards,
                actions,
                clock,
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
            return Some(StateValue::Parallel {
                regions: new_regions,
            });
        }

        let current_state = StateValue::Parallel {
            regions: regions.to_vec(),
        };
        let path = self.build_parallel_full_path(parallel_node, regions);
        self.send_along_path(
            &path,
            &current_state,
            event,
            event_payload,
            context,
            guards,
            actions,
            clock,
        )
    }

    fn build_parallel_full_path<'a>(
        &self,
        parallel_node: &'a StateNode,
        regions: &[(String, StateValue)],
    ) -> Vec<&'a StateNode> {
        let mut path = vec![parallel_node];
        for (region_name, region_value) in regions {
            if let Some(region_node) = parallel_node
                .children
                .iter()
                .find(|n| &n.name == region_name)
            {
                path.push(region_node);
                path.extend(self.find_full_path_inner(region_value, &region_node.children));
            }
        }
        path
    }

    #[allow(clippy::too_many_arguments)]
    fn send_scoped(
        &self,
        state: &StateValue,
        search_scope: &[StateNode],
        transition_scope: &[StateNode],
        event: &str,
        event_payload: &serde_json::Value,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
        clock: &dyn Clock,
    ) -> Option<StateValue> {
        match state {
            StateValue::Atomic(name) => {
                let node = search_scope
                    .iter()
                    .find(|n| n.name == *name)
                    .or_else(|| transition_scope.iter().find(|n| n.name == *name))
                    .or_else(|| Self::find_node_deep(search_scope, name))
                    .or_else(|| Self::find_node_deep(transition_scope, name))?;
                self.check_node_transitions(
                    node,
                    state,
                    transition_scope,
                    event,
                    event_payload,
                    context,
                    guards,
                    actions,
                    clock,
                )
            }
            StateValue::Compound {
                state: compound_name,
                child,
            } => {
                let node = search_scope
                    .iter()
                    .find(|n| n.name == *compound_name)
                    .or_else(|| Self::find_node_deep(search_scope, compound_name))?;
                if let Some(result) = self.check_node_transitions(
                    node,
                    state,
                    transition_scope,
                    event,
                    event_payload,
                    context,
                    guards,
                    actions,
                    clock,
                ) {
                    return Some(result);
                }
                let new_child = self.send_scoped(
                    child,
                    &node.children,
                    &node.children,
                    event,
                    event_payload,
                    context,
                    guards,
                    actions,
                    clock,
                )?;
                Some(StateValue::Compound {
                    state: compound_name.clone(),
                    child: Box::new(new_child),
                })
            }
            _ => None,
        }
    }

    fn find_node_deep<'a>(states: &'a [StateNode], name: &str) -> Option<&'a StateNode> {
        for state in states {
            if state.name == name {
                return Some(state);
            }
            if let Some(found) = Self::find_node_deep(&state.children, name) {
                return Some(found);
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn check_node_transitions(
        &self,
        node: &StateNode,
        current_value: &StateValue,
        transition_scope: &[StateNode],
        event: &str,
        event_payload: &serde_json::Value,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
        clock: &dyn Clock,
    ) -> Option<StateValue> {
        if node.node_type == StateType::Final {
            return None;
        }
        for transition in &node.transitions {
            if transition.event != event {
                continue;
            }
            if let Some(ref guard) = transition.guard
                && !guards(&guard.name, context, event_payload)
            {
                continue;
            }
            if let Some(ref target_name) = transition.target {
                let target = transition_scope
                    .iter()
                    .find(|n| n.name == *target_name)
                    .or_else(|| Self::find_node_deep(transition_scope, target_name))?;
                let target_value = self.resolve_initial_value(target);
                let now = clock.now_ms();
                self.entry_times
                    .borrow_mut()
                    .insert(target.name.clone(), now);
                for action_name in &node.exit {
                    run_action_by_name(action_name, event_payload, context, actions);
                }
                for action in &transition.actions {
                    run_action(action, event_payload, context, actions);
                }
                for action_name in &target.entry {
                    run_action_by_name(action_name, event_payload, context, actions);
                }
                return Some(target_value);
            } else {
                for action in &transition.actions {
                    run_action(action, event_payload, context, actions);
                }
                return Some(current_value.clone());
            }
        }
        None
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

    #[allow(clippy::too_many_arguments)]
    fn send_along_path(
        &self,
        path: &[&StateNode],
        current_state: &StateValue,
        event: &str,
        event_payload: &serde_json::Value,
        context: &mut serde_json::Value,
        guards: &dyn Fn(&str, &serde_json::Value, &serde_json::Value) -> bool,
        actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value),
        clock: &dyn Clock,
    ) -> Option<StateValue> {
        for node in path.iter().rev() {
            if node.node_type == StateType::Final {
                continue;
            }
            for transition in &node.transitions {
                if transition.event != event {
                    continue;
                }
                if let Some(ref guard) = transition.guard
                    && !guards(&guard.name, context, event_payload)
                {
                    continue;
                }

                if !transition.targets.is_empty() {
                    let (target_path, target_value) =
                        self.build_multi_target(&transition.targets)?;

                    let common_prefix = path
                        .iter()
                        .zip(target_path.iter())
                        .take_while(|(s, t)| s.name == t.name)
                        .count();

                    if common_prefix < path.len() {
                        for i in common_prefix..path.len() {
                            if i > 0 {
                                self.record_history_for_node(path[i - 1], path);
                            }
                        }
                    }

                    let now = clock.now_ms();
                    for n in &target_path[common_prefix..] {
                        self.entry_times.borrow_mut().insert(n.name.clone(), now);
                    }

                    for n in path[common_prefix..].iter().rev() {
                        for action_name in &n.exit {
                            run_action_by_name(action_name, event_payload, context, actions);
                        }
                    }

                    for action in &transition.actions {
                        run_action(action, event_payload, context, actions);
                    }

                    for n in &target_path[common_prefix..] {
                        for action_name in &n.entry {
                            run_action_by_name(action_name, event_payload, context, actions);
                        }
                    }

                    return Some(target_value);
                } else if let Some(ref target_name) = transition.target {
                    let (target_path, target_value) = self.build_target(target_name)?;

                    let common_prefix = path
                        .iter()
                        .zip(target_path.iter())
                        .take_while(|(s, t)| s.name == t.name)
                        .count();

                    if common_prefix < path.len() {
                        for i in common_prefix..path.len() {
                            if i > 0 {
                                self.record_history_for_node(path[i - 1], path);
                            }
                        }
                    }

                    for i in (0..common_prefix.min(path.len())).rev() {
                        let parent = path[i];
                        let child_in_path = path.get(i + 1);
                        if let Some(child) = child_in_path {
                            let has_history = parent.children.iter().any(|c| {
                                c.node_type == StateType::History
                                    || c.node_type == StateType::DeepHistory
                            });
                            if has_history {
                                let value = self.resolve_initial_value(child);
                                self.history.borrow_mut().insert(parent.name.clone(), value);
                            }
                        }
                    }

                    let now = clock.now_ms();
                    for n in &target_path[common_prefix..] {
                        self.entry_times.borrow_mut().insert(n.name.clone(), now);
                    }

                    for n in path[common_prefix..].iter().rev() {
                        for action_name in &n.exit {
                            run_action_by_name(action_name, event_payload, context, actions);
                        }
                    }

                    for action in &transition.actions {
                        run_action(action, event_payload, context, actions);
                    }

                    for n in &target_path[common_prefix..] {
                        for action_name in &n.entry {
                            run_action_by_name(action_name, event_payload, context, actions);
                        }
                    }

                    return Some(target_value);
                } else {
                    for action in &transition.actions {
                        run_action(action, event_payload, context, actions);
                    }
                    return Some(current_state.clone());
                }
            }
        }

        None
    }

    fn build_target(&self, target_name: &str) -> Option<(Vec<&StateNode>, StateValue)> {
        if target_name.contains('.') {
            let parts: Vec<&str> = target_name.split('.').collect();
            return self.build_target_from_path(&parts);
        }

        let (path_to_named, leaf) = Self::find_node_by_name_in(&self.machine.states, target_name)?;

        if (leaf.node_type == StateType::History || leaf.node_type == StateType::DeepHistory)
            && path_to_named.len() >= 2
        {
            let ancestors = &path_to_named[..path_to_named.len() - 1];
            let is_deep = leaf.node_type == StateType::DeepHistory;
            return Some(self.resolve_history_target(ancestors, is_deep));
        }

        let value = self.state_value_from_path(&path_to_named);

        let mut full_path: Vec<&StateNode> = path_to_named.clone();
        self.expand_initial_path(leaf, &mut full_path);

        Some((full_path, value))
    }

    fn build_target_from_path(&self, parts: &[&str]) -> Option<(Vec<&StateNode>, StateValue)> {
        let first = parts.first()?;
        let root = self.machine.states.iter().find(|s| s.name == *first)?;
        let mut path: Vec<&StateNode> = vec![root];
        for part in &parts[1..] {
            let parent = *path.last()?;
            let child = parent.children.iter().find(|c| c.name == *part)?;
            path.push(child);
        }
        let leaf = *path.last()?;

        if (leaf.node_type == StateType::History || leaf.node_type == StateType::DeepHistory)
            && path.len() >= 2
        {
            let ancestors = &path[..path.len() - 1];
            let is_deep = leaf.node_type == StateType::DeepHistory;
            return Some(self.resolve_history_target(ancestors, is_deep));
        }

        let value = self.state_value_from_path(&path);
        let mut full_path = path.clone();
        self.expand_initial_path(leaf, &mut full_path);
        Some((full_path, value))
    }

    fn build_multi_target(&self, target_names: &[String]) -> Option<(Vec<&StateNode>, StateValue)> {
        let mut results: Vec<(Vec<&StateNode>, StateValue)> = Vec::new();
        for name in target_names {
            results.push(self.build_target(name)?);
        }
        if results.is_empty() {
            return None;
        }

        let first_path = &results[0].0;
        let mut common_prefix_len = first_path.len();
        for (path, _) in &results[1..] {
            let mut i = 0;
            while i < common_prefix_len && i < path.len() && first_path[i].name == path[i].name {
                i += 1;
            }
            common_prefix_len = i;
        }

        let common_ancestor_path = first_path[..common_prefix_len].to_vec();
        let mut full_path = common_ancestor_path.clone();

        let mut regions = Vec::new();
        for (path, _) in &results {
            let region_name = path[common_prefix_len].name.clone();
            let region_node = path[common_prefix_len];
            let region_value = self.resolve_initial_value(region_node);
            full_path.push(region_node);
            let mut region_full_path = vec![region_node];
            self.expand_initial_path(region_node, &mut region_full_path);
            for n in &region_full_path[1..] {
                full_path.push(*n);
            }
            regions.push((region_name, region_value));
        }

        let parallel_child = StateValue::Parallel { regions };
        let value = if common_ancestor_path.is_empty() {
            parallel_child
        } else {
            let mut value = parallel_child;
            for ancestor in common_ancestor_path.iter().rev() {
                value = StateValue::Compound {
                    state: ancestor.name.clone(),
                    child: Box::new(value),
                };
            }
            value
        };

        Some((full_path, value))
    }

    fn resolve_history_target<'a>(
        &self,
        ancestors: &[&'a StateNode],
        is_deep: bool,
    ) -> (Vec<&'a StateNode>, StateValue) {
        let parent = *ancestors.last().expect("ancestors should not be empty");
        // xstate ignores shallow history states as direct children of parallel
        // states: the parallel state re-enters its initial configuration.
        if parent.node_type == StateType::Parallel && !is_deep {
            let value = self.state_value_from_path(ancestors);
            let mut full_path = ancestors.to_vec();
            self.expand_initial_path(parent, &mut full_path);
            return (full_path, value);
        }
        let recorded = self.history.borrow().get(&parent.name).cloned();
        if let Some(recorded) = recorded {
            let mut value = recorded.clone();
            for ancestor in ancestors.iter().rev() {
                value = StateValue::Compound {
                    state: ancestor.name.clone(),
                    child: Box::new(value),
                };
            }
            (ancestors.to_vec(), value)
        } else {
            let value = self.state_value_from_path(ancestors);
            let mut full_path = ancestors.to_vec();
            self.expand_initial_path(parent, &mut full_path);
            (full_path, value)
        }
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
        if node.node_type == StateType::Compound
            && let Some(initial_name) = &node.initial
            && let Some(child) = node.children.iter().find(|c| &c.name == initial_name)
        {
            path.push(child);
            self.expand_initial_path(child, path);
        }
    }

    fn resolve_initial_value(&self, node: &StateNode) -> StateValue {
        match node.node_type {
            StateType::Compound => {
                let has_history_child = node.children.iter().any(|c| {
                    c.node_type == StateType::History || c.node_type == StateType::DeepHistory
                });

                let result = if let Some(initial_name) = &node.initial {
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
                };

                if has_history_child && let StateValue::Compound { child, .. } = &result {
                    self.history
                        .borrow_mut()
                        .insert(node.name.clone(), *child.clone());
                }

                result
            }
            StateType::Parallel => {
                let regions: Vec<_> = node
                    .children
                    .iter()
                    .filter(|c| {
                        c.node_type != StateType::History && c.node_type != StateType::DeepHistory
                    })
                    .map(|c| (c.name.clone(), self.resolve_region_value(c)))
                    .collect();
                StateValue::Compound {
                    state: node.name.clone(),
                    child: Box::new(StateValue::Parallel { regions }),
                }
            }
            StateType::History | StateType::DeepHistory => self
                .history
                .borrow()
                .get(&node.name)
                .cloned()
                .unwrap_or(StateValue::Atomic(node.name.clone())),
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

    fn record_history_for_node(&self, node: &StateNode, full_path: &[&StateNode]) {
        let has_history = node
            .children
            .iter()
            .any(|c| c.node_type == StateType::History || c.node_type == StateType::DeepHistory);
        if !has_history {
            return;
        }

        let node_idx = full_path.iter().position(|n| n.name == node.name);
        let Some(idx) = node_idx else { return };

        if let Some(child) = full_path.get(idx + 1) {
            let has_deep = node
                .children
                .iter()
                .any(|c| c.node_type == StateType::DeepHistory);
            if has_deep {
                let value = self.compute_history_value(node, full_path, idx);
                self.history.borrow_mut().insert(node.name.clone(), value);
            } else {
                let value = self.resolve_initial_value(child);
                self.history.borrow_mut().insert(node.name.clone(), value);
            }
        }
    }

    fn compute_history_value(
        &self,
        node: &StateNode,
        path: &[&StateNode],
        node_idx: usize,
    ) -> StateValue {
        if node.node_type == StateType::Parallel {
            let regions: Vec<_> = node
                .children
                .iter()
                .filter(|c| {
                    c.node_type != StateType::History && c.node_type != StateType::DeepHistory
                })
                .map(|region| {
                    let value = match path.iter().position(|n| n.name == region.name) {
                        Some(ridx) => self.compute_history_value(region, path, ridx),
                        None => self.resolve_region_value(region),
                    };
                    (region.name.clone(), value)
                })
                .collect();
            return StateValue::Parallel { regions };
        }
        if let Some(child) = path.get(node_idx + 1) {
            // The child is a leaf when the path ends here or the next path
            // node belongs to a different subtree (e.g. a sibling region).
            let child_is_leaf = match path.get(node_idx + 2) {
                None => true,
                Some(next) => Self::find_node_deep(&child.children, &next.name).is_none(),
            };
            if child_is_leaf {
                return StateValue::Atomic(child.name.clone());
            }
            let child_value = self.compute_history_value(child, path, node_idx + 1);
            if child.name != node.name {
                StateValue::Compound {
                    state: child.name.clone(),
                    child: Box::new(child_value),
                }
            } else {
                child_value
            }
        } else {
            StateValue::Atomic(node.name.clone())
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
    use crate::clock::MockClock;
    use xstate_ir::{
        Action, ContextField, ContextSchema, Guard, InvokeConfig, StateNode as IrStateNode,
        StateType, Transition,
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
            ],

            setup: None,
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

        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_name: &str, _ctx: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "toggle"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(new_state, StateValue::Atomic("active".into()));
        assert!(actions_log.contains(&"increment".to_string()));
    }

    #[test]
    fn no_matching_event_returns_none() {
        let interpreter = Interpreter::new(toggle_machine());
        let state = StateValue::Atomic("inactive".into());
        let mut context = interpreter.initial_context();

        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let result = interpreter.send(
            &state,
            &serde_json::json!({"type": "unknown"}),
            &mut context,
            &guards,
            &mut actions,
            &MockClock::new(0),
        );
        assert!(result.is_none());
    }

    #[test]
    fn guard_false_skips_transition() {
        let interpreter = Interpreter::new(payment_machine());
        let state = StateValue::Atomic("idle".into());
        let mut context = interpreter.initial_context();

        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| false;

        let result = interpreter.send(
            &state,
            &serde_json::json!({"type": "PAY"}),
            &mut context,
            &guards,
            &mut actions,
            &MockClock::new(0),
        );
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
                    ],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
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
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
            ],

            setup: None,
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

        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "PAY"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
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

        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &processing_auth,
                &serde_json::json!({"type": "AUTH_OK"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
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

        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &processing_capture,
                &serde_json::json!({"type": "CAPTURE_OK"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
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

        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let result = interpreter.send(
            &done_state,
            &serde_json::json!({"type": "CAPTURE_OK"}),
            &mut context,
            &guards,
            &mut actions,
            &MockClock::new(0),
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
                    delay_ms: None,
                    targets: vec![],
                }],
                children: vec![],
                node_type: StateType::Atomic,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
            }],

            setup: None,
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("idle".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let result = interpreter.send(
            &state,
            &serde_json::json!({"type": "LOG"}),
            &mut context,
            &guards,
            &mut actions,
            &MockClock::new(0),
        );
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

        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "DONE"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
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
                        invoke: None,
                        on_done: vec![],
                        output: None,
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
                        invoke: None,
                        on_done: vec![],
                        output: None,
                    },
                ],
                node_type: StateType::Parallel,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
            }],

            setup: None,
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
                        invoke: None,
                        on_done: vec![],
                        output: None,
                    }],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                }],
                node_type: StateType::Compound,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
            }],

            setup: None,
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
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
            ],

            setup: None,
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("start".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<(String, Vec<serde_json::Value>)> = Vec::new();

        let mut actions = |name: &str, args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push((name.to_string(), args.to_vec()));
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        interpreter
            .send(
                &state,
                &serde_json::json!({"type": "GO"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
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
                        delay_ms: None,
                        targets: vec![],
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
                        invoke: None,
                        on_done: vec![],
                        output: None,
                    }],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
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
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
            ],

            setup: None,
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Compound {
            state: "outer".into(),
            child: Box::new(StateValue::Atomic("inner".into())),
        };
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        interpreter
            .send(
                &state,
                &serde_json::json!({"type": "NEXT"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
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
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
            ],

            setup: None,
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("a".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        interpreter
            .send(
                &state,
                &serde_json::json!({"type": "NEXT"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
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
            ],

            setup: None,
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("a".into());
        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();

        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        interpreter
            .send(
                &state,
                &serde_json::json!({"type": "NEXT"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
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

    #[test]
    fn always_transition_fires_after_regular_transition() {
        let machine = Machine {
            id: "alwaysChain".into(),
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
                        event: "START".into(),
                        target: Some("auto".into()),
                        guard: None,
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
                IrStateNode {
                    name: "auto".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "".into(),
                        target: Some("done".into()),
                        guard: None,
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
                IrStateNode {
                    name: "done".into(),
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

        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        let mut context = interpreter.initial_context();
        assert_eq!(state, StateValue::Atomic("start".into()));

        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "START"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(new_state, StateValue::Atomic("done".into()));
    }

    #[test]
    fn always_transition_with_guard() {
        let machine = Machine {
            id: "alwaysGuard".into(),
            initial: "start".into(),
            context: ContextSchema {
                fields: vec![ContextField {
                    name: "ready".into(),
                    type_name: "boolean".into(),
                    default_value: serde_json::json!(false),
                }],
            },
            states: vec![
                IrStateNode {
                    name: "start".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "GO".into(),
                        target: Some("check".into()),
                        guard: None,
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
                IrStateNode {
                    name: "check".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "".into(),
                        target: Some("done".into()),
                        guard: Some(Guard {
                            name: "ready".into(),
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
                IrStateNode {
                    name: "done".into(),
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

        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        let mut context = interpreter.initial_context();

        assert_eq!(state, StateValue::Atomic("start".into()));

        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |name: &str, ctx: &serde_json::Value, _: &serde_json::Value| {
            name == "ready" && ctx.get("ready").and_then(|v| v.as_bool()).unwrap_or(false)
        };

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "GO"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();

        assert_eq!(new_state, StateValue::Atomic("check".into()));
    }

    #[test]
    fn persistence_round_trip() {
        let interpreter = Interpreter::new(toggle_machine());
        let state = interpreter.initial_state();
        let ctx = interpreter.initial_context();

        let state_json = serde_json::to_value(&state).unwrap();
        let snapshot_json = serde_json::json!({
            "value": state_json,
            "context": ctx
        });

        let restored_state: StateValue =
            serde_json::from_value(snapshot_json["value"].clone()).unwrap();
        let restored_ctx: serde_json::Value = snapshot_json["context"].clone();

        let (r_state, r_ctx) = interpreter.restore(&restored_state, restored_ctx);
        assert_eq!(r_state, StateValue::Atomic("inactive".into()));

        let mut context = r_ctx;
        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &r_state,
                &serde_json::json!({"type": "toggle"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(new_state, StateValue::Atomic("active".into()));
    }

    #[test]
    fn shallow_history_remembers_last_child() {
        let machine = Machine {
            id: "history".into(),
            initial: "main".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "main".into(),
                    description: None,
                    initial: Some("idle".into()),
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "SWITCH".into(),
                        target: Some("other".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    }],
                    children: vec![
                        IrStateNode {
                            name: "idle".into(),
                            node_type: StateType::Atomic,
                            transitions: vec![Transition {
                                event: "NEXT".into(),
                                target: Some("active".into()),
                                guard: None,
                                actions: vec![],
                                description: None,
                                delay_ms: None,
                                targets: vec![],
                            }],
                            children: vec![],
                            entry: vec![],
                            exit: vec![],
                            tags: vec![],
                            invoke: None,
                            on_done: vec![],
                            output: None,
                            description: None,
                            initial: None,
                        },
                        IrStateNode {
                            name: "active".into(),
                            node_type: StateType::Atomic,
                            transitions: vec![],
                            children: vec![],
                            entry: vec![],
                            exit: vec![],
                            tags: vec![],
                            invoke: None,
                            on_done: vec![],
                            output: None,
                            description: None,
                            initial: None,
                        },
                        IrStateNode {
                            name: "hist".into(),
                            node_type: StateType::History,
                            transitions: vec![],
                            children: vec![],
                            entry: vec![],
                            exit: vec![],
                            tags: vec![],
                            invoke: None,
                            on_done: vec![],
                            output: None,
                            description: None,
                            initial: None,
                        },
                    ],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
                IrStateNode {
                    name: "other".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "BACK".into(),
                        target: Some("hist".into()),
                        guard: None,
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
            ],

            setup: None,
        };

        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        assert_eq!(
            state,
            StateValue::Compound {
                state: "main".into(),
                child: Box::new(StateValue::Atomic("idle".into())),
            }
        );

        let mut context = interpreter.initial_context();
        // Transition inside main: idle → NEXT → active
        let state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "NEXT"}),
                &mut context,
                &|_, _, _| true,
                &mut |_, _, _| {},
                &MockClock::new(0),
            )
            .unwrap();

        // Now leave main: main → SWITCH → other
        let state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "SWITCH"}),
                &mut context,
                &|_, _, _| true,
                &mut |_, _, _| {},
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(state, StateValue::Atomic("other".into()));

        // Now go back: other → BACK → main
        // History should restore to "active" (the last active child before leaving)
        let state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "BACK"}),
                &mut context,
                &|_, _, _| true,
                &mut |_, _, _| {},
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(
            state,
            StateValue::Compound {
                state: "main".into(),
                child: Box::new(StateValue::Atomic("active".into())),
            }
        );
    }

    #[test]
    fn invoke_done_transition_fires() {
        let machine = Machine {
            id: "invokeTest".into(),
            initial: "loading".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "loading".into(),
                    node_type: StateType::Atomic,
                    transitions: vec![Transition {
                        event: "done.invoke.loader".into(),
                        target: Some("loaded".into()),
                        delay_ms: None,
                        targets: vec![],
                        guard: None,
                        actions: vec![],
                        description: None,
                    }],
                    invoke: Some(InvokeConfig {
                        src: "fetchData".into(),
                        id: Some("loader".into()),
                        on_done: vec![],
                        on_error: vec![],
                    }),
                    on_done: vec![],
                    output: None,
                    children: vec![],
                    entry: vec![],
                    exit: vec![],
                    tags: vec![],
                    description: None,
                    initial: None,
                },
                IrStateNode {
                    name: "loaded".into(),
                    node_type: StateType::Atomic,
                    transitions: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                    children: vec![],
                    entry: vec![],
                    exit: vec![],
                    tags: vec![],
                    description: None,
                    initial: None,
                },
            ],

            setup: None,
        };
        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        let mut context = interpreter.initial_context();

        assert_eq!(state, StateValue::Atomic("loading".into()));

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "done.invoke.loader"}),
                &mut context,
                &|_, _, _| true,
                &mut |_, _, _| {},
                &MockClock::new(0),
            )
            .unwrap();

        assert_eq!(new_state, StateValue::Atomic("loaded".into()));
    }

    fn history_machine() -> Machine {
        Machine {
            id: "m".into(),
            initial: "on".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "on".into(),
                    description: None,
                    initial: Some("first".into()),
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "POWER".into(),
                        target: Some("off".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    }],
                    children: vec![
                        IrStateNode {
                            name: "first".into(),
                            description: None,
                            initial: None,
                            entry: vec![],
                            exit: vec![],
                            transitions: vec![Transition {
                                event: "SWITCH".into(),
                                target: Some("second".into()),
                                guard: None,
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
                        IrStateNode {
                            name: "second".into(),
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
                        IrStateNode {
                            name: "hist".into(),
                            description: None,
                            initial: None,
                            entry: vec![],
                            exit: vec![],
                            transitions: vec![],
                            children: vec![],
                            node_type: StateType::History,
                            tags: vec![],
                            invoke: None,
                            on_done: vec![],
                            output: None,
                        },
                    ],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
                IrStateNode {
                    name: "off".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "POWER".into(),
                        target: Some("on.hist".into()),
                        guard: None,
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
            ],

            setup: None,
        }
    }

    #[test]
    fn dot_path_history_restore() {
        let interpreter = Interpreter::new(history_machine());
        let mut context = interpreter.initial_context();
        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let state = interpreter.initial_state();
        assert_eq!(
            state,
            StateValue::Compound {
                state: "on".into(),
                child: Box::new(StateValue::Atomic("first".into())),
            }
        );

        let state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "SWITCH"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(
            state,
            StateValue::Compound {
                state: "on".into(),
                child: Box::new(StateValue::Atomic("second".into())),
            }
        );

        let state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "POWER"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(state, StateValue::Atomic("off".into()));

        let state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "POWER"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(
            state,
            StateValue::Compound {
                state: "on".into(),
                child: Box::new(StateValue::Atomic("second".into())),
            }
        );
    }

    #[test]
    fn dot_path_simple_target() {
        let machine = Machine {
            id: "dot".into(),
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
                        target: Some("parent.b".into()),
                        guard: None,
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
                IrStateNode {
                    name: "parent".into(),
                    description: None,
                    initial: Some("a".into()),
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![
                        IrStateNode {
                            name: "a".into(),
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
                        IrStateNode {
                            name: "b".into(),
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
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
            ],

            setup: None,
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("start".into());
        let mut context = interpreter.initial_context();
        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "GO"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(
            new_state,
            StateValue::Compound {
                state: "parent".into(),
                child: Box::new(StateValue::Atomic("b".into())),
            }
        );
    }

    #[test]
    fn nested_dot_path() {
        let machine = Machine {
            id: "nested".into(),
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
                        target: Some("a.b.c".into()),
                        guard: None,
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
                IrStateNode {
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
                            invoke: None,
                            on_done: vec![],
                            output: None,
                        }],
                        node_type: StateType::Compound,
                        tags: vec![],
                        invoke: None,
                        on_done: vec![],
                        output: None,
                    }],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
            ],

            setup: None,
        };

        let interpreter = Interpreter::new(machine);
        let state = StateValue::Atomic("start".into());
        let mut context = interpreter.initial_context();
        let mut actions = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "GO"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();
        assert_eq!(
            new_state,
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
    fn parallel_node_own_transition() {
        let machine = Machine {
            id: "m".into(),
            initial: "on".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "on".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec!["exit_on".into()],
                    transitions: vec![Transition {
                        event: "POWER".into(),
                        target: Some("off".into()),
                        guard: None,
                        actions: vec![Action {
                            name: "power_off".into(),
                            args: vec![],
                        }],
                        description: None,
                        delay_ms: None,
                        targets: vec![],
                    }],
                    children: vec![
                        IrStateNode {
                            name: "a".into(),
                            description: None,
                            initial: Some("a1".into()),
                            entry: vec![],
                            exit: vec!["exit_a".into()],
                            transitions: vec![],
                            children: vec![
                                IrStateNode {
                                    name: "a1".into(),
                                    description: None,
                                    initial: None,
                                    entry: vec![],
                                    exit: vec!["exit_a1".into()],
                                    transitions: vec![],
                                    children: vec![],
                                    node_type: StateType::Atomic,
                                    tags: vec![],
                                    invoke: None,
                                    on_done: vec![],
                                    output: None,
                                },
                                IrStateNode {
                                    name: "a2".into(),
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
                            node_type: StateType::Compound,
                            tags: vec![],
                            invoke: None,
                            on_done: vec![],
                            output: None,
                        },
                        IrStateNode {
                            name: "b".into(),
                            description: None,
                            initial: Some("b1".into()),
                            entry: vec![],
                            exit: vec!["exit_b".into()],
                            transitions: vec![],
                            children: vec![
                                IrStateNode {
                                    name: "b1".into(),
                                    description: None,
                                    initial: None,
                                    entry: vec![],
                                    exit: vec!["exit_b1".into()],
                                    transitions: vec![],
                                    children: vec![],
                                    node_type: StateType::Atomic,
                                    tags: vec![],
                                    invoke: None,
                                    on_done: vec![],
                                    output: None,
                                },
                                IrStateNode {
                                    name: "b2".into(),
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
                            node_type: StateType::Compound,
                            tags: vec![],
                            invoke: None,
                            on_done: vec![],
                            output: None,
                        },
                    ],
                    node_type: StateType::Parallel,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                },
                IrStateNode {
                    name: "off".into(),
                    description: None,
                    initial: None,
                    entry: vec!["enter_off".into()],
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

        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        assert_eq!(
            state,
            StateValue::Parallel {
                regions: vec![
                    ("a".into(), StateValue::Atomic("a1".into())),
                    ("b".into(), StateValue::Atomic("b1".into())),
                ],
            }
        );

        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();
        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "POWER"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();

        assert_eq!(new_state, StateValue::Atomic("off".into()));
        assert!(actions_log.contains(&"exit_a1".to_string()));
        assert!(actions_log.contains(&"exit_a".to_string()));
        assert!(actions_log.contains(&"exit_b1".to_string()));
        assert!(actions_log.contains(&"exit_b".to_string()));
        assert!(actions_log.contains(&"exit_on".to_string()));
        assert!(actions_log.contains(&"power_off".to_string()));
        assert!(actions_log.contains(&"enter_off".to_string()));
    }

    #[test]
    fn parallel_nested_compound_transition() {
        let machine = Machine {
            id: "m".into(),
            initial: "outer".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![IrStateNode {
                name: "outer".into(),
                description: None,
                initial: None,
                entry: vec![],
                exit: vec![],
                transitions: vec![],
                children: vec![IrStateNode {
                    name: "region".into(),
                    description: None,
                    initial: Some("nested".into()),
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![],
                    children: vec![IrStateNode {
                        name: "nested".into(),
                        description: None,
                        initial: Some("deep".into()),
                        entry: vec![],
                        exit: vec![],
                        transitions: vec![],
                        children: vec![
                            IrStateNode {
                                name: "deep".into(),
                                description: None,
                                initial: None,
                                entry: vec![],
                                exit: vec!["exit_deep".into()],
                                transitions: vec![Transition {
                                    event: "GO".into(),
                                    target: Some("elsewhere".into()),
                                    guard: None,
                                    actions: vec![Action {
                                        name: "go".into(),
                                        args: vec![],
                                    }],
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
                            IrStateNode {
                                name: "elsewhere".into(),
                                description: None,
                                initial: None,
                                entry: vec!["enter_elsewhere".into()],
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
                        node_type: StateType::Compound,
                        tags: vec![],
                        invoke: None,
                        on_done: vec![],
                        output: None,
                    }],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![],
                    output: None,
                }],
                node_type: StateType::Parallel,
                tags: vec![],
                invoke: None,
                on_done: vec![],
                output: None,
            }],

            setup: None,
        };

        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        assert_eq!(
            state,
            StateValue::Parallel {
                regions: vec![(
                    "region".into(),
                    StateValue::Compound {
                        state: "nested".into(),
                        child: Box::new(StateValue::Atomic("deep".into())),
                    },
                )],
            }
        );

        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();
        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "GO"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();

        assert_eq!(
            new_state,
            StateValue::Parallel {
                regions: vec![(
                    "region".into(),
                    StateValue::Compound {
                        state: "nested".into(),
                        child: Box::new(StateValue::Atomic("elsewhere".into())),
                    },
                )],
            }
        );
        assert!(actions_log.contains(&"exit_deep".to_string()));
        assert!(actions_log.contains(&"go".to_string()));
        assert!(actions_log.contains(&"enter_elsewhere".to_string()));
    }

    #[test]
    fn ondone_compound_completion() {
        let machine = Machine {
            id: "completion".into(),
            initial: "loading".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                IrStateNode {
                    name: "loading".into(),
                    description: None,
                    initial: Some("fetch".into()),
                    entry: vec![],
                    exit: vec!["exit_loading".into()],
                    transitions: vec![],
                    children: vec![
                        IrStateNode {
                            name: "fetch".into(),
                            description: None,
                            initial: None,
                            entry: vec![],
                            exit: vec![],
                            transitions: vec![Transition {
                                event: "DONE".into(),
                                target: Some("complete".into()),
                                guard: None,
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
                        IrStateNode {
                            name: "complete".into(),
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
                        },
                    ],
                    node_type: StateType::Compound,
                    tags: vec![],
                    invoke: None,
                    on_done: vec![Transition {
                        event: String::new(),
                        target: Some("loaded".into()),
                        targets: vec![],
                        guard: None,
                        actions: vec![Action {
                            name: "on_loading_done".into(),
                            args: vec![],
                        }],
                        description: None,
                        delay_ms: None,
                    }],
                    output: None,
                },
                IrStateNode {
                    name: "loaded".into(),
                    description: None,
                    initial: None,
                    entry: vec!["enter_loaded".into()],
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

        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();

        let mut context = interpreter.initial_context();
        let mut actions_log: Vec<String> = Vec::new();
        let mut actions = |name: &str, _args: &[serde_json::Value], _: &serde_json::Value| {
            actions_log.push(name.to_string());
        };
        let guards = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;

        let new_state = interpreter
            .send(
                &state,
                &serde_json::json!({"type": "DONE"}),
                &mut context,
                &guards,
                &mut actions,
                &MockClock::new(0),
            )
            .unwrap();

        assert_eq!(new_state, StateValue::Atomic("loaded".into()));
        assert!(actions_log.contains(&"exit_loading".to_string()));
        assert!(actions_log.contains(&"on_loading_done".to_string()));
        assert!(actions_log.contains(&"enter_loaded".to_string()));
    }
}
