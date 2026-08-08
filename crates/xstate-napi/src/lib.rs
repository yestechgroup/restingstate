use napi_derive::napi;
use std::cell::RefCell;
use xstate_core::{Interpreter, StateValue};
use xstate_ir::Machine;

#[napi]
pub struct JsMachine {
    interpreter: Interpreter,
    state: RefCell<StateValue>,
    context: RefCell<serde_json::Value>,
}

#[napi]
impl JsMachine {
    #[napi(constructor)]
    pub fn new(machine_json: String) -> napi::Result<Self> {
        let machine: Machine = serde_json::from_str(&machine_json)
            .map_err(|e| napi::Error::from_reason(format!("Invalid machine JSON: {}", e)))?;
        let interpreter = Interpreter::new(machine.clone());
        let state = interpreter.initial_state();
        let context = interpreter.initial_context();
        Ok(JsMachine {
            interpreter,
            state: RefCell::new(state),
            context: RefCell::new(context),
        })
    }

    #[napi(factory)]
    pub fn from_ir(machine_json: String, context_json: String) -> napi::Result<Self> {
        let machine: Machine = serde_json::from_str(&machine_json)
            .map_err(|e| napi::Error::from_reason(format!("Invalid machine JSON: {}", e)))?;
        let context: serde_json::Value = serde_json::from_str(&context_json)
            .map_err(|e| napi::Error::from_reason(format!("Invalid context JSON: {}", e)))?;
        let interpreter = Interpreter::new(machine);
        let state = interpreter.initial_state();
        Ok(JsMachine {
            interpreter,
            state: RefCell::new(state),
            context: RefCell::new(context),
        })
    }

    #[napi]
    pub fn snapshot(&self) -> napi::Result<String> {
        let state_value = serde_json::to_value(&*self.state.borrow())
            .map_err(|e| napi::Error::from_reason(format!("Serialize error: {}", e)))?;
        let snapshot = serde_json::json!({
            "value": state_value,
            "context": self.context.borrow().clone(),
        });
        serde_json::to_string(&snapshot)
            .map_err(|e| napi::Error::from_reason(format!("Serialize error: {}", e)))
    }

    #[napi]
    pub fn send(&self, event_json: String) -> napi::Result<Option<String>> {
        let event: serde_json::Value = serde_json::from_str(&event_json)
            .map_err(|e| napi::Error::from_reason(format!("Invalid event JSON: {}", e)))?;
        let event_type = event["type"]
            .as_str()
            .ok_or_else(|| napi::Error::from_reason("Event must have a 'type' field"))?;

        let mut context = self.context.borrow_mut();
        let current_state = self.state.borrow().clone();

        let guards = |name: &str, ctx: &serde_json::Value| -> bool {
            if let Some(val) = ctx.get(name) {
                matches!(val, serde_json::Value::Bool(true))
            } else {
                true
            }
        };

        let mut actions = |_name: &str, _args: &[serde_json::Value]| {};

        match self.interpreter.send(
            &current_state,
            event_type,
            &mut context,
            &guards,
            &mut actions,
        ) {
            Some(new_state) => {
                *self.state.borrow_mut() = new_state;
                let state_value = serde_json::to_value(&*self.state.borrow())
                    .map_err(|e| napi::Error::from_reason(format!("Serialize error: {}", e)))?;
                let snapshot = serde_json::json!({
                    "value": state_value,
                    "context": context.clone(),
                });
                let json = serde_json::to_string(&snapshot)
                    .map_err(|e| napi::Error::from_reason(format!("Serialize error: {}", e)))?;
                Ok(Some(json))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xstate_ir::{ContextSchema, Machine, StateNode, StateType, Transition};

    fn toggle_machine_json() -> String {
        let machine = Machine {
            id: "toggle".into(),
            initial: "inactive".into(),
            context: ContextSchema { fields: vec![] },
            states: vec![
                StateNode {
                    name: "inactive".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "TOGGLE".into(),
                        target: Some("active".into()),
                        guard: None,
                        actions: vec![],
                        description: None,
                    }],
                    children: vec![],
                    node_type: StateType::Atomic,
                    tags: vec![],
                },
                StateNode {
                    name: "active".into(),
                    description: None,
                    initial: None,
                    entry: vec![],
                    exit: vec![],
                    transitions: vec![Transition {
                        event: "TOGGLE".into(),
                        target: Some("inactive".into()),
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
        serde_json::to_string(&machine).unwrap()
    }

    #[test]
    fn test_construct_toggle_machine() {
        let json = toggle_machine_json();
        let machine = JsMachine::new(json).unwrap();
        let snapshot = machine.snapshot().unwrap();
        let snap: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snap["value"], serde_json::json!("inactive"));
    }

    #[test]
    fn test_initial_snapshot() {
        let json = toggle_machine_json();
        let machine = JsMachine::new(json).unwrap();
        let snapshot = machine.snapshot().unwrap();
        let snap: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snap["context"], serde_json::json!({}));
    }

    #[test]
    fn test_send_transition() {
        let json = toggle_machine_json();
        let machine = JsMachine::new(json).unwrap();
        let event = serde_json::json!({"type": "TOGGLE"}).to_string();
        let result = machine.send(event).unwrap().unwrap();
        let snap: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(snap["value"], serde_json::json!("active"));
    }

    #[test]
    fn test_send_unknown_event() {
        let json = toggle_machine_json();
        let machine = JsMachine::new(json).unwrap();
        let event = serde_json::json!({"type": "UNKNOWN"}).to_string();
        let result = machine.send(event).unwrap();
        assert!(result.is_none());
    }
}
