use std::collections::HashMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::error::ParseError;
use xstate_ir::{
    Action, ContextField, ContextSchema, Guard, Machine, StateNode, StateType, Transition,
};

pub fn parse(source: &str) -> Result<Machine, ParseError> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

    if !ret.errors.is_empty() {
        return Err(ParseError::OxcParse);
    }

    extract_machine(&ret.program)
}

fn extract_machine(program: &Program) -> Result<Machine, ParseError> {
    for stmt in program.body.iter() {
        match find_create_machine_in_stmt(stmt) {
            SearchResult::Found(config, delays) => {
                return extract_machine_from_config(config, &delays)
            }
            SearchResult::NotFound => {}
        }
    }

    Err(ParseError::InvalidMachine(
        "No createMachine call found".into(),
    ))
}

enum SearchResult<'a> {
    Found(&'a ObjectExpression<'a>, HashMap<String, u64>),
    NotFound,
}

fn find_create_machine_in_stmt<'a>(stmt: &'a Statement<'a>) -> SearchResult<'a> {
    match stmt {
        // `createMachine({...})` as standalone expression
        Statement::ExpressionStatement(es) => find_create_machine_in_expr(&es.expression),

        // `const x = createMachine({...})`
        Statement::VariableDeclaration(vd) => {
            for decl in vd.declarations.iter() {
                if let Some(init) = &decl.init {
                    match find_create_machine_in_expr(init) {
                        SearchResult::Found(config, delays) => {
                            return SearchResult::Found(config, delays)
                        }
                        SearchResult::NotFound => {}
                    }
                }
            }
            SearchResult::NotFound
        }

        // `export const x = createMachine({...})`
        Statement::ExportNamedDeclaration(end) => {
            if let Some(declaration) = &end.declaration {
                find_create_machine_in_declaration(declaration)
            } else {
                SearchResult::NotFound
            }
        }

        // `export default createMachine({...})`
        Statement::ExportDefaultDeclaration(edd) => {
            find_create_machine_in_export_default_kind(&edd.declaration)
        }

        _ => SearchResult::NotFound,
    }
}

fn find_create_machine_in_declaration<'a>(decl: &'a Declaration<'a>) -> SearchResult<'a> {
    match decl {
        Declaration::VariableDeclaration(vd) => {
            for declarator in vd.declarations.iter() {
                if let Some(init) = &declarator.init {
                    match find_create_machine_in_expr(init) {
                        SearchResult::Found(config, delays) => {
                            return SearchResult::Found(config, delays)
                        }
                        SearchResult::NotFound => {}
                    }
                }
            }
            SearchResult::NotFound
        }
        _ => SearchResult::NotFound,
    }
}

#[allow(dead_code)]
fn find_create_machine_in_export_default_kind<'a>(
    kind: &'a ExportDefaultDeclarationKind<'a>,
) -> SearchResult<'a> {
    if let Some(expr) = kind.as_expression() {
        find_create_machine_in_expr(expr)
    } else {
        SearchResult::NotFound
    }
}

fn find_create_machine_in_expr<'a>(expr: &'a Expression<'a>) -> SearchResult<'a> {
    if let Some((config, delays)) = try_extract_create_machine_config(expr) {
        return SearchResult::Found(config, delays);
    }
    SearchResult::NotFound
}

fn try_extract_create_machine_config<'a>(
    expr: &'a Expression<'a>,
) -> Option<(&'a ObjectExpression<'a>, HashMap<String, u64>)> {
    let call = match expr {
        Expression::CallExpression(call) => call,
        _ => return None,
    };

    let is_create = match &call.callee {
        Expression::Identifier(ident) => ident.name.as_ref() == "createMachine",
        Expression::StaticMemberExpression(member) => {
            member.property.name.as_ref() == "createMachine"
        }
        _ => false,
    };

    if !is_create {
        return None;
    }

    let config = call
        .arguments
        .first()
        .and_then(|arg| arg.as_expression())
        .and_then(|expr| match expr {
            Expression::ObjectExpression(obj) => Some(obj.as_ref()),
            _ => None,
        })?;

    let delays = if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::CallExpression(setup_call) = &member.object {
            if let Expression::Identifier(ident) = &setup_call.callee {
                if ident.name.as_ref() == "setup" {
                    extract_delays_from_setup_params(setup_call)
                } else {
                    HashMap::new()
                }
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    Some((config, delays))
}

fn extract_machine_from_config(
    obj: &ObjectExpression,
    delays: &HashMap<String, u64>,
) -> Result<Machine, ParseError> {
    let id = extract_string_prop(obj, "id")
        .ok_or_else(|| ParseError::InvalidMachine("Machine missing 'id'".into()))?;

    let initial = extract_string_prop(obj, "initial")
        .ok_or_else(|| ParseError::InvalidMachine("Machine missing 'initial'".into()))?;

    let context = obj
        .get_object_prop_value("context")
        .and_then(|expr| match expr {
            Expression::ObjectExpression(ctx_obj) => Some(extract_context_schema(ctx_obj)),
            _ => None,
        })
        .unwrap_or_else(|| ContextSchema { fields: vec![] });

    let states = obj
        .get_object_prop_value("states")
        .and_then(|expr| match expr {
            Expression::ObjectExpression(states_obj) => {
                Some(extract_states_object(states_obj, delays))
            }
            _ => None,
        })
        .transpose()?
        .unwrap_or_default();

    Ok(Machine {
        id,
        initial,
        context,
        states,
    })
}

fn extract_delays_from_setup_params(call: &CallExpression) -> HashMap<String, u64> {
    let mut delays = HashMap::new();

    let setup_config = call.arguments.first().and_then(|arg| arg.as_expression());

    let setup_config = match setup_config {
        Some(Expression::ObjectExpression(obj)) => obj,
        _ => return delays,
    };

    let delays_obj = match setup_config.get_object_prop_value("delays") {
        Some(Expression::ObjectExpression(obj)) => obj,
        _ => return delays,
    };

    for prop in delays_obj.properties.iter() {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let name = property_key_name(&p.key).map(|n| n.to_string());
            let delay_ms = match &p.value {
                Expression::NumericLiteral(n) => Some(n.value as u64),
                _ => None,
            };
            if let (Some(name), Some(ms)) = (name, delay_ms) {
                delays.insert(name, ms);
            }
        }
    }

    delays
}

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

fn extract_string(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::TemplateLiteral(t) => t.quasis.first().map(|q| q.value.raw.to_string()),
        _ => None,
    }
}

fn extract_string_prop(obj: &ObjectExpression, key: &str) -> Option<String> {
    let expr = obj.get_object_prop_value(key)?;
    extract_string(expr)
}

#[allow(dead_code)]
fn extract_string_array(arr: &ArrayExpression) -> Vec<String> {
    arr.elements
        .iter()
        .filter_map(|elem| {
            let expr = elem.as_expression()?;
            extract_string(expr)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Object property helpers
// ---------------------------------------------------------------------------

fn property_key_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    if let PropertyKey::StaticIdentifier(id) = key {
        return Some(id.name.as_ref());
    }
    if let PropertyKey::StringLiteral(s) = key {
        return Some(s.value.as_ref());
    }
    if let PropertyKey::NumericLiteral(n) = key {
        if let Some(ref raw) = n.raw {
            return Some(raw.as_ref());
        }
    }
    None
}

/// Extension method to get the value expression of an object property by name.
trait ObjectExpressionExt {
    fn get_object_prop_value(&self, key: &str) -> Option<&Expression<'_>>;
}

impl<'a> ObjectExpressionExt for ObjectExpression<'a> {
    fn get_object_prop_value(&self, key: &str) -> Option<&Expression<'a>> {
        for prop in self.properties.iter() {
            if let ObjectPropertyKind::ObjectProperty(p) = prop {
                if let Some(name) = property_key_name(&p.key) {
                    if name == key {
                        return Some(&p.value);
                    }
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Context schema
// ---------------------------------------------------------------------------

fn extract_context_schema(obj: &ObjectExpression) -> ContextSchema {
    let mut fields = Vec::new();

    for prop in obj.properties.iter() {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let name = match property_key_name(&p.key) {
                Some(n) => n.to_string(),
                None => continue,
            };

            let (type_name, default_value) = expr_to_type_and_value(&p.value);

            fields.push(ContextField {
                name,
                type_name,
                default_value,
            });
        }
    }

    ContextSchema { fields }
}

fn expr_to_type_and_value(expr: &Expression) -> (String, serde_json::Value) {
    match expr {
        Expression::StringLiteral(s) => (
            "string".into(),
            serde_json::Value::String(s.value.to_string()),
        ),
        Expression::NumericLiteral(n) => {
            let val = n.value;
            if val.fract() == 0.0 && val >= i64::MIN as f64 && val <= i64::MAX as f64 {
                ("number".into(), serde_json::json!(val as i64))
            } else {
                ("number".into(), serde_json::json!(val))
            }
        }
        Expression::BooleanLiteral(b) => ("boolean".into(), serde_json::Value::Bool(b.value)),
        Expression::NullLiteral(_) => ("null".into(), serde_json::Value::Null),
        Expression::ArrayExpression(a) => {
            let vals: Vec<serde_json::Value> = a
                .elements
                .iter()
                .filter_map(|elem| {
                    let expr = elem.as_expression()?;
                    Some(expr_to_type_and_value(expr).1)
                })
                .collect();
            ("array".into(), serde_json::Value::Array(vals))
        }
        Expression::ObjectExpression(o) => {
            let mut map = serde_json::Map::new();
            for p in o.properties.iter() {
                if let ObjectPropertyKind::ObjectProperty(op) = p {
                    let key = match property_key_name(&op.key) {
                        Some(k) => k.to_string(),
                        None => continue,
                    };
                    let (_, val) = expr_to_type_and_value(&op.value);
                    map.insert(key, val);
                }
            }
            ("object".into(), serde_json::Value::Object(map))
        }
        _ => ("unknown".into(), serde_json::Value::Null),
    }
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

fn extract_states_object(
    obj: &ObjectExpression,
    delays: &HashMap<String, u64>,
) -> Result<Vec<StateNode>, ParseError> {
    let mut states = Vec::new();

    for prop in obj.properties.iter() {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let name = match property_key_name(&p.key) {
                Some(n) => n.to_string(),
                None => continue,
            };

            if let Expression::ObjectExpression(state_obj) = &p.value {
                let state = extract_state_node(&name, state_obj, delays)?;
                states.push(state);
            }
        }
    }

    Ok(states)
}

fn extract_state_node(
    name: &str,
    obj: &ObjectExpression,
    delays: &HashMap<String, u64>,
) -> Result<StateNode, ParseError> {
    let description = extract_string_prop(obj, "description");

    let node_type = extract_string_prop(obj, "type")
        .map(|s| parse_state_type(&s))
        .unwrap_or_default();

    let initial = extract_string_prop(obj, "initial");

    let entry = obj
        .get_object_prop_value("entry")
        .and_then(|expr| match expr {
            Expression::ArrayExpression(arr) => Some(extract_string_array(arr)),
            _ => None,
        })
        .unwrap_or_default();

    let exit = obj
        .get_object_prop_value("exit")
        .and_then(|expr| match expr {
            Expression::ArrayExpression(arr) => Some(extract_string_array(arr)),
            _ => None,
        })
        .unwrap_or_default();

    let transitions = obj
        .get_object_prop_value("on")
        .and_then(|expr| match expr {
            Expression::ObjectExpression(on_obj) => Some(extract_transitions(on_obj)),
            _ => None,
        })
        .transpose()?
        .unwrap_or_default();

    // always transitions (eventless) — extract from array
    let always_transitions: Vec<Transition> = obj
        .get_object_prop_value("always")
        .and_then(|expr| match expr {
            Expression::ArrayExpression(arr) => Some(extract_always_transitions(arr)),
            _ => None,
        })
        .transpose()?
        .unwrap_or_default();

    // after (delayed) transitions
    let after_transitions: Vec<Transition> = obj
        .get_object_prop_value("after")
        .and_then(|expr| match expr {
            Expression::ObjectExpression(after_obj) => {
                Some(extract_after_transitions(after_obj, delays))
            }
            _ => None,
        })
        .transpose()?
        .unwrap_or_default();

    let children = obj
        .get_object_prop_value("states")
        .and_then(|expr| match expr {
            Expression::ObjectExpression(children_obj) => {
                Some(extract_states_object(children_obj, delays))
            }
            _ => None,
        })
        .transpose()?
        .unwrap_or_default();

    let tags = obj
        .get_object_prop_value("tags")
        .and_then(|expr| match expr {
            Expression::ArrayExpression(arr) => Some(extract_string_array(arr)),
            _ => None,
        })
        .unwrap_or_default();

    let mut all_transitions = transitions;
    all_transitions.extend(always_transitions);
    all_transitions.extend(after_transitions);

    Ok(StateNode {
        name: name.to_string(),
        description,
        initial,
        entry,
        exit,
        transitions: all_transitions,
        children,
        node_type,
        tags,
    })
}

fn parse_state_type(s: &str) -> StateType {
    match s {
        "atomic" => StateType::Atomic,
        "compound" => StateType::Compound,
        "parallel" => StateType::Parallel,
        "final" => StateType::Final,
        "history" => StateType::History,
        "deepHistory" | "deephistory" => StateType::DeepHistory,
        _ => StateType::Atomic,
    }
}

// ---------------------------------------------------------------------------
// Transitions
// ---------------------------------------------------------------------------

fn extract_transitions(on_obj: &ObjectExpression) -> Result<Vec<Transition>, ParseError> {
    let mut transitions = Vec::new();

    for prop in on_obj.properties.iter() {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let event = match property_key_name(&p.key) {
                Some(n) => n.to_string(),
                None => continue,
            };

            let mut event_transitions = extract_transition_value(&event, &p.value)?;
            transitions.append(&mut event_transitions);
        }
    }

    Ok(transitions)
}

fn extract_transition_value(
    event: &str,
    value: &Expression,
) -> Result<Vec<Transition>, ParseError> {
    match value {
        // Simple string target: `on: { TOGGLE: 'active' }`
        Expression::StringLiteral(s) => Ok(vec![Transition {
            event: event.to_string(),
            target: Some(s.value.to_string()),
            guard: None,
            delay_ms: None,
            actions: vec![],
            description: None,
        }]),
        // Array: `on: { SUBMIT: [{ target: 'success', guard: 'isValid' }, { target: 'error' }] }`
        Expression::ArrayExpression(arr) => {
            let mut transitions = Vec::new();
            for elem in arr.elements.iter() {
                if let Some(elem_expr) = elem.as_expression() {
                    let t = extract_single_transition(event, elem_expr);
                    transitions.push(t);
                }
            }
            Ok(transitions)
        }
        // Object: `on: { SUBMIT: { target: 'processing', actions: ['log'] } }`
        Expression::ObjectExpression(obj) => Ok(vec![extract_transition_from_obj(event, obj)]),
        _ => Ok(vec![]),
    }
}

fn extract_single_transition(event: &str, expr: &Expression) -> Transition {
    match expr {
        // String inside array: `[{ target: 'a' }, 'b']` -> simple target
        Expression::StringLiteral(s) => Transition {
            event: event.to_string(),
            target: Some(s.value.to_string()),
            guard: None,
            delay_ms: None,
            actions: vec![],
            description: None,
        },
        Expression::ObjectExpression(obj) => extract_transition_from_obj(event, obj),
        _ => Transition {
            event: event.to_string(),
            target: None,
            guard: None,
            delay_ms: None,
            actions: vec![],
            description: None,
        },
    }
}

fn extract_transition_from_obj(event: &str, obj: &ObjectExpression) -> Transition {
    let target = extract_string_prop(obj, "target");

    let guard = obj
        .get_object_prop_value("guard")
        .and_then(extract_guard_expr);

    let actions = obj
        .get_object_prop_value("actions")
        .map(extract_actions_expr)
        .unwrap_or_default();

    let description = extract_string_prop(obj, "description");

    Transition {
        event: event.to_string(),
        target,
        guard,
        actions,
        description,
        delay_ms: None,
    }
}

fn extract_always_transitions(arr: &ArrayExpression) -> Result<Vec<Transition>, ParseError> {
    let mut transitions = Vec::new();

    for elem in arr.elements.iter() {
        if let Some(elem_expr) = elem.as_expression() {
            match elem_expr {
                Expression::ObjectExpression(obj) => {
                    let t = extract_transition_from_obj("", obj);
                    transitions.push(t);
                }
                Expression::StringLiteral(s) => {
                    transitions.push(Transition {
                        event: String::new(),
                        target: Some(s.value.to_string()),
                        guard: None,
                        delay_ms: None,
                        actions: vec![],
                        description: None,
                    });
                }
                _ => {}
            }
        }
    }

    Ok(transitions)
}

fn extract_after_transitions(
    after_obj: &ObjectExpression,
    delays: &HashMap<String, u64>,
) -> Result<Vec<Transition>, ParseError> {
    let mut transitions = Vec::new();

    for prop in after_obj.properties.iter() {
        let ObjectPropertyKind::ObjectProperty(ref obj_prop) = prop else {
            continue;
        };

        let key_name = property_key_name(&obj_prop.key);

        let delay_ms = if let Some(name) = key_name {
            if let Ok(ms) = name.parse::<u64>() {
                Some(ms)
            } else {
                delays.get(name).copied()
            }
        } else {
            None
        };

        let event_transitions = extract_transition_value("", &obj_prop.value)?;
        for mut t in event_transitions {
            t.delay_ms = delay_ms;
            transitions.push(t);
        }
    }

    Ok(transitions)
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

fn extract_guard_expr(expr: &Expression) -> Option<Guard> {
    // Simple string guard: `guard: 'isValid'`
    if let Some(s) = extract_string(expr) {
        return Some(Guard { name: s });
    }

    // Object guard: `guard: { type: 'isValid' }`
    if let Expression::ObjectExpression(obj) = expr {
        if let Some(name) = extract_string_prop(obj, "type") {
            return Some(Guard { name });
        }
        if let Some(name) = extract_string_prop(obj, "name") {
            return Some(Guard { name });
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

fn extract_actions_expr(expr: &Expression) -> Vec<Action> {
    match expr {
        // Single string action: `actions: 'log'`
        Expression::StringLiteral(s) => {
            vec![Action {
                name: s.value.to_string(),
                args: vec![],
            }]
        }
        // Array: `actions: ['log', { type: 'assign', count: 1 }]`
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|elem| extract_action_from_element(elem))
            .collect(),
        _ => vec![],
    }
}

fn extract_action_from_element(elem: &ArrayExpressionElement) -> Option<Action> {
    let expr = elem.as_expression()?;
    match expr {
        Expression::StringLiteral(s) => Some(Action {
            name: s.value.to_string(),
            args: vec![],
        }),
        Expression::ObjectExpression(obj) => {
            let name = extract_string_prop(obj, "type")?;
            let args: Vec<serde_json::Value> = obj
                .properties
                .iter()
                .filter_map(|p| {
                    if let ObjectPropertyKind::ObjectProperty(op) = p {
                        let key = property_key_name(&op.key)?;
                        if key == "type" || key == "name" {
                            return None;
                        }
                        let (_, val) = expr_to_type_and_value(&op.value);
                        Some(val)
                    } else {
                        None
                    }
                })
                .collect();
            Some(Action { name, args })
        }
        _ => None,
    }
}
