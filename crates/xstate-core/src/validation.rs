//! Machine structural validation + the `validate_machine_json` boundary entry
//! point (#30). Shared by `Interpreter::validate_machine` and the WASM
//! `validateMachine` export so all boundaries check the same rules.

use xstate_ir::{Machine, StateNode, StateType};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MachineValidationError {
    #[error("initial state '{0}' not found in machine states")]
    UnknownInitialState(String),
    #[error("compound state '{state}' declares unknown initial child '{child}'")]
    UnknownInitialChild { state: String, child: String },
    #[error("state '{state}' transition targets unknown state '{target}'")]
    UnknownTarget { state: String, target: String },
}

/// Typed failure of [`validate_machine_json`]: the payload was not machine IR,
/// or it parsed but the machine is structurally broken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidateError {
    #[error("machine IR is not valid JSON: {0}")]
    Parse(String),
    #[error(transparent)]
    Invalid(#[from] MachineValidationError),
}

/// Parse `json` as an `xstate-ir` [`Machine`] and run the full structural
/// validation (initial resolvability, transition targets exist).
pub fn validate_machine_json(json: &str) -> Result<(), ValidateError> {
    let machine: Machine =
        serde_json::from_str(json).map_err(|e| ValidateError::Parse(e.to_string()))?;
    validate_machine_structure(&machine)?;
    Ok(())
}

/// Structural validation shared by `Interpreter::validate_machine` and
/// `validate_machine_json` (#30).
pub fn validate_machine_structure(machine: &Machine) -> Result<(), MachineValidationError> {
    if !machine.states.iter().any(|s| s.name == machine.initial) {
        return Err(MachineValidationError::UnknownInitialState(
            machine.initial.clone(),
        ));
    }
    for state in &machine.states {
        validate_state_node(state, &machine.states)?;
    }
    Ok(())
}

fn validate_state_node(
    node: &StateNode,
    roots: &[StateNode],
) -> Result<(), MachineValidationError> {
    if node.node_type == StateType::Compound
        && let Some(initial) = &node.initial
        && !node.children.iter().any(|c| &c.name == initial)
    {
        return Err(MachineValidationError::UnknownInitialChild {
            state: node.name.clone(),
            child: initial.clone(),
        });
    }
    let transitions = node
        .transitions
        .iter()
        .chain(&node.on_done)
        .chain(node.invoke.iter().flat_map(|inv| inv.on_done.iter()))
        .chain(node.invoke.iter().flat_map(|inv| inv.on_error.iter()));
    for transition in transitions {
        for target in transition.target.iter().chain(&transition.targets) {
            if !target_exists(roots, target) {
                return Err(MachineValidationError::UnknownTarget {
                    state: node.name.clone(),
                    target: target.clone(),
                });
            }
        }
    }
    for child in &node.children {
        validate_state_node(child, roots)?;
    }
    Ok(())
}

// Mirrors `Interpreter::build_target` resolution: dotted targets are
// root-anchored paths, bare names are searched anywhere in the tree.
fn target_exists(roots: &[StateNode], target: &str) -> bool {
    if target.contains('.') {
        let mut parts = target.split('.');
        let Some(first) = parts.next() else {
            return false;
        };
        let Some(mut current) = roots.iter().find(|s| s.name == first) else {
            return false;
        };
        for part in parts {
            match current.children.iter().find(|c| c.name == part) {
                Some(child) => current = child,
                None => return false,
            }
        }
        true
    } else {
        find_node_deep(roots, target)
    }
}

fn find_node_deep(states: &[StateNode], name: &str) -> bool {
    states
        .iter()
        .any(|s| s.name == name || find_node_deep(&s.children, name))
}
