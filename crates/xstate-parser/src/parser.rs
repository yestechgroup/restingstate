use std::collections::HashMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::error::ParseError;
use xstate_ir::{
    Action, ContextField, ContextSchema, Guard, InvokeConfig, Machine, StateNode, StateType,
    Transition,
};

pub fn parse(source: &str) -> Result<Machine, ParseError> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

    if !ret.errors.is_empty() {
        return Err(ParseError::OxcParse);
    }

    extract_machine(&ret.program)
}

/// Parse a TypeScript source and return ALL machines found in it.
/// Unlike `parse()`, this does not stop at the first createMachine call.
pub fn parse_all(source: &str) -> Result<Vec<Machine>, ParseError> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

    if !ret.errors.is_empty() {
        return Err(ParseError::OxcParse);
    }

    collect_all_machines(&ret.program)
}

fn collect_all_machines(program: &Program) -> Result<Vec<Machine>, ParseError> {
    let mut machines = Vec::new();
    for stmt in program.body.iter() {
        collect_from_statement(stmt, &mut machines)?;
    }
    Ok(machines)
}

fn collect_from_statement<'a>(
    stmt: &'a Statement<'a>,
    machines: &mut Vec<Machine>,
) -> Result<(), ParseError> {
    match stmt {
        // `createMachine({...})` or `describe(() => { createMachine(...) })`
        Statement::ExpressionStatement(expr_stmt) => {
            collect_from_expression(&expr_stmt.expression, machines)?;
        }
        // `const x = createMachine({...})`
        Statement::VariableDeclaration(var_decl) => {
            for decl in var_decl.declarations.iter() {
                if let Some(init) = &decl.init {
                    collect_from_expression(init, machines)?;
                }
            }
        }
        // `export const x = createMachine({...})`
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = &export.declaration {
                collect_from_declaration(decl, machines)?;
            }
        }
        // `export default createMachine({...})`
        Statement::ExportDefaultDeclaration(export) => {
            if let Some(expr) = export.declaration.as_expression() {
                collect_from_expression(expr, machines)?;
            }
        }
        // `function foo() { createMachine({...}) }`
        Statement::FunctionDeclaration(func) => {
            if let Some(body) = &func.body {
                collect_from_function_body(body, machines)?;
            }
        }
        // `return createMachine({...})`
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                collect_from_expression(arg, machines)?;
            }
        }
        // `{ ... }` blocks (bodies of if/loops are also Statements)
        Statement::BlockStatement(block) => {
            for stmt in block.body.iter() {
                collect_from_statement(stmt, machines)?;
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_from_statement(&if_stmt.consequent, machines)?;
            if let Some(alternate) = &if_stmt.alternate {
                collect_from_statement(alternate, machines)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_from_declaration<'a>(
    decl: &'a Declaration<'a>,
    machines: &mut Vec<Machine>,
) -> Result<(), ParseError> {
    match decl {
        Declaration::VariableDeclaration(var_decl) => {
            for declarator in var_decl.declarations.iter() {
                if let Some(init) = &declarator.init {
                    collect_from_expression(init, machines)?;
                }
            }
        }
        Declaration::FunctionDeclaration(func) => {
            if let Some(body) = &func.body {
                collect_from_function_body(body, machines)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_from_function_body<'a>(
    body: &'a FunctionBody<'a>,
    machines: &mut Vec<Machine>,
) -> Result<(), ParseError> {
    for stmt in body.statements.iter() {
        collect_from_statement(stmt, machines)?;
    }
    Ok(())
}

fn collect_from_expression<'a>(
    expr: &'a Expression<'a>,
    machines: &mut Vec<Machine>,
) -> Result<(), ParseError> {
    match expr {
        Expression::CallExpression(call) => collect_from_call(call, machines),
        // Optional chaining: `foo?.createMachine({...})`
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => collect_from_call(call, machines),
            ChainElement::TSNonNullExpression(non_null) => {
                collect_from_expression(&non_null.expression, machines)
            }
            _ => Ok(()),
        },
        // `(createMachine({...}))`
        Expression::ParenthesizedExpression(paren) => {
            collect_from_expression(&paren.expression, machines)
        }
        // `() => { const m = createMachine({...}); }`
        Expression::ArrowFunctionExpression(arrow) => {
            collect_from_function_body(&arrow.body, machines)
        }
        // `function () { const m = createMachine({...}); }`
        Expression::FunctionExpression(func) => {
            if let Some(body) = &func.body {
                collect_from_function_body(body, machines)?;
            }
            Ok(())
        }
        // `{ on: { EVENT: () => createMachine({...}) } }`
        Expression::ObjectExpression(obj) => {
            for prop in obj.properties.iter() {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    collect_from_expression(&p.value, machines)?;
                }
            }
            Ok(())
        }
        Expression::ArrayExpression(arr) => {
            for elem in arr.elements.iter() {
                if let Some(elem_expr) = elem.as_expression() {
                    collect_from_expression(elem_expr, machines)?;
                }
            }
            Ok(())
        }
        Expression::StaticMemberExpression(member) => {
            collect_from_expression(&member.object, machines)
        }
        // TypeScript wrappers: `createMachine({...}) as const`, `satisfies`, `!`, `<T>`
        Expression::TSAsExpression(as_expr) => {
            collect_from_expression(&as_expr.expression, machines)
        }
        Expression::TSSatisfiesExpression(sat) => {
            collect_from_expression(&sat.expression, machines)
        }
        Expression::TSTypeAssertion(assertion) => {
            collect_from_expression(&assertion.expression, machines)
        }
        Expression::TSNonNullExpression(non_null) => {
            collect_from_expression(&non_null.expression, machines)
        }
        _ => Ok(()),
    }
}

fn collect_from_call<'a>(
    call: &'a CallExpression<'a>,
    machines: &mut Vec<Machine>,
) -> Result<(), ParseError> {
    if let Some((config, delays)) = try_get_machine_config(call) {
        push_machine_lenient(config, &delays, machines);
    }
    // Recurse into arguments (they may contain nested createMachine calls)
    for arg in call.arguments.iter() {
        collect_from_argument(arg, machines)?;
    }
    Ok(())
}

fn collect_from_argument<'a>(
    arg: &'a Argument<'a>,
    machines: &mut Vec<Machine>,
) -> Result<(), ParseError> {
    match arg {
        Argument::SpreadElement(spread) => collect_from_expression(&spread.argument, machines),
        _ => {
            if let Some(expr) = arg.as_expression() {
                collect_from_expression(expr, machines)?;
            }
            Ok(())
        }
    }
}

/// Extract every `createMachine(...)` call from a file, in source order.
///
/// Unlike [`parse`], this walks the whole program (including machines nested
/// inside `it(...)`/`describe(...)` callback bodies) and is lenient per
/// machine: a machine missing `id`/`initial` gets a synthesized id and its
/// first state as initial, matching XState's default behaviour.
fn push_machine_lenient(
    config: &ObjectExpression,
    delays: &HashMap<String, u64>,
    out: &mut Vec<Machine>,
) {
    let id = extract_string_prop(config, "id").unwrap_or_else(|| format!("machine-{}", out.len()));

    let states = config
        .get_object_prop_value("states")
        .and_then(|expr| match expr {
            Expression::ObjectExpression(states_obj) => {
                extract_states_object(states_obj, delays).ok()
            }
            _ => None,
        })
        .unwrap_or_default();

    // A root-parallel machine has no single initial state; leave the initial
    // empty so callers can detect and skip it (the IR cannot express a
    // parallel root, and neither runtime can run one faithfully).
    let root_type = extract_string_prop(config, "type");
    let initial = if root_type.as_deref() == Some("parallel") {
        extract_string_prop(config, "initial").unwrap_or_default()
    } else {
        extract_string_prop(config, "initial")
            .or_else(|| states.first().map(|s| s.name.clone()))
            .unwrap_or_default()
    };

    let context = config
        .get_object_prop_value("context")
        .and_then(|expr| match expr {
            Expression::ObjectExpression(ctx_obj) => Some(extract_context_schema(ctx_obj)),
            _ => None,
        })
        .unwrap_or_else(|| ContextSchema { fields: vec![] });

    let mut machine = Machine {
        id,
        initial,
        context,
        states,
    };
    infer_state_types(&mut machine.states);
    out.push(machine);
}

/// xstate.js infers a state to be compound when it declares `initial` or
/// children even without an explicit `type: 'compound'`. Mirror that so the
/// runtime resolves nested states correctly.
fn infer_state_types(states: &mut [StateNode]) {
    for state in states.iter_mut() {
        if state.node_type == StateType::Atomic
            && (state.initial.is_some() || !state.children.is_empty())
        {
            state.node_type = StateType::Compound;
        }
        infer_state_types(&mut state.children);
    }
}

fn extract_machine(program: &Program) -> Result<Machine, ParseError> {
    for stmt in program.body.iter() {
        match find_create_machine_in_stmt(stmt) {
            SearchResult::Found(config, delays) => {
                return extract_machine_from_config(config, &delays);
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
                            return SearchResult::Found(config, delays);
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
                            return SearchResult::Found(config, delays);
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
    try_get_machine_config(call)
}

fn try_get_machine_config<'a>(
    call: &'a CallExpression<'a>,
) -> Option<(&'a ObjectExpression<'a>, HashMap<String, u64>)> {
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
            match expr {
                Expression::CallExpression(call) if is_assign_call(call) => {
                    Some("assign".to_string())
                }
                _ => extract_string(expr),
            }
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
    if let PropertyKey::NumericLiteral(n) = key
        && let Some(ref raw) = n.raw
    {
        return Some(raw.as_ref());
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
            if let ObjectPropertyKind::ObjectProperty(p) = prop
                && let Some(name) = property_key_name(&p.key)
                && name == key
            {
                return Some(&p.value);
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

    let on_done: Vec<Transition> = obj
        .get_object_prop_value("onDone")
        .map(|expr| -> Result<Vec<Transition>, ParseError> {
            match expr {
                Expression::StringLiteral(s) => Ok(vec![Transition {
                    event: String::new(),
                    target: Some(s.value.to_string()),
                    targets: vec![],
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                }]),
                Expression::ObjectExpression(on_done_obj) => {
                    Ok(vec![extract_transition_from_obj("", on_done_obj)])
                }
                Expression::ArrayExpression(arr) => {
                    let mut transitions = Vec::new();
                    for elem in arr.elements.iter() {
                        if let Some(elem_expr) = elem.as_expression() {
                            match elem_expr {
                                Expression::StringLiteral(s) => {
                                    transitions.push(Transition {
                                        event: String::new(),
                                        target: Some(s.value.to_string()),
                                        targets: vec![],
                                        guard: None,
                                        actions: vec![],
                                        description: None,
                                        delay_ms: None,
                                    });
                                }
                                Expression::ObjectExpression(obj) => {
                                    transitions.push(extract_transition_from_obj("", obj));
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(transitions)
                }
                _ => Ok(vec![]),
            }
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

    let invoke = obj
        .get_object_prop_value("invoke")
        .map(|expr| match expr {
            Expression::ObjectExpression(invoke_obj) => extract_invoke_config(invoke_obj),
            _ => Err(ParseError::InvalidMachine(
                "invoke must be an object".into(),
            )),
        })
        .transpose()?;

    let tags = obj
        .get_object_prop_value("tags")
        .and_then(|expr| match expr {
            Expression::ArrayExpression(arr) => Some(extract_string_array(arr)),
            _ => None,
        })
        .unwrap_or_default();

    // `history: true` / `history: 'deep'` / `history: 'shallow'` shorthand.
    // Only apply when the state has no children — a compound state with
    // `history: true` is a compound state with metadata, not a history
    // pseudostate.
    let node_type = if node_type == StateType::Atomic && children.is_empty() {
        match obj.get_object_prop_value("history") {
            Some(Expression::BooleanLiteral(b)) if b.value => StateType::History,
            Some(Expression::StringLiteral(s)) => {
                if s.value == "deep" {
                    StateType::DeepHistory
                } else {
                    StateType::History
                }
            }
            _ => node_type,
        }
    } else {
        node_type
    };

    let mut all_transitions = transitions;
    all_transitions.extend(always_transitions);
    all_transitions.extend(after_transitions);
    // Merge invoke on_done/on_error into state transitions so the
    // interpreter finds them via send_along_path.
    if let Some(ref invoke_config) = invoke {
        for t in &invoke_config.on_done {
            all_transitions.push(t.clone());
        }
        for t in &invoke_config.on_error {
            all_transitions.push(t.clone());
        }
    }

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
        invoke,
        on_done,
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
            targets: vec![],
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
            targets: vec![],
            actions: vec![],
            description: None,
        },
        Expression::ObjectExpression(obj) => extract_transition_from_obj(event, obj),
        _ => Transition {
            event: event.to_string(),
            target: None,
            guard: None,
            delay_ms: None,
            targets: vec![],
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
        targets: vec![],
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
                        targets: vec![],
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
        let ObjectPropertyKind::ObjectProperty(obj_prop) = prop else {
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
// Invoke
// ---------------------------------------------------------------------------

fn extract_invoke_config(invoke_obj: &ObjectExpression) -> Result<InvokeConfig, ParseError> {
    let src = extract_string_prop(invoke_obj, "src")
        .ok_or_else(|| ParseError::InvalidMachine("invoke missing 'src'".into()))?;

    let id = extract_string_prop(invoke_obj, "id");

    let on_done = invoke_obj
        .get_object_prop_value("onDone")
        .map(|v| -> Result<Vec<Transition>, ParseError> {
            match v {
                Expression::StringLiteral(s) => Ok(vec![Transition {
                    event: format!("done.invoke.{}", id.as_deref().unwrap_or(&src)),
                    target: Some(s.value.to_string()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }]),
                Expression::ObjectExpression(obj) => {
                    let t = extract_transition_from_obj(
                        &format!("done.invoke.{}", id.as_deref().unwrap_or(&src)),
                        obj,
                    );
                    Ok(vec![t])
                }
                _ => Err(ParseError::InvalidMachine("Invalid onDone value".into())),
            }
        })
        .transpose()?
        .unwrap_or_default();

    let on_error = invoke_obj
        .get_object_prop_value("onError")
        .map(|v| -> Result<Vec<Transition>, ParseError> {
            match v {
                Expression::StringLiteral(s) => Ok(vec![Transition {
                    event: format!("error.platform.{}", id.as_deref().unwrap_or(&src)),
                    target: Some(s.value.to_string()),
                    guard: None,
                    actions: vec![],
                    description: None,
                    delay_ms: None,
                    targets: vec![],
                }]),
                Expression::ObjectExpression(obj) => {
                    let t = extract_transition_from_obj(
                        &format!("error.platform.{}", id.as_deref().unwrap_or(&src)),
                        obj,
                    );
                    Ok(vec![t])
                }
                _ => Err(ParseError::InvalidMachine("Invalid onError value".into())),
            }
        })
        .transpose()?
        .unwrap_or_default();

    Ok(InvokeConfig {
        src,
        id,
        on_done,
        on_error,
    })
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
        Expression::StringLiteral(s) => {
            vec![Action {
                name: s.value.to_string(),
                args: vec![],
            }]
        }
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|elem| extract_action_from_element(elem))
            .collect(),
        Expression::CallExpression(call) => {
            if is_assign_call(call) {
                extract_assign_action(call)
                    .map(|a| vec![a])
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }
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
        Expression::CallExpression(call) if is_assign_call(call) => extract_assign_action(call),
        _ => None,
    }
}

fn is_assign_call(call: &CallExpression) -> bool {
    match &call.callee {
        // assign(...)
        callee if callee.is_specific_id("assign") => true,
        _ => false,
    }
}

fn extract_assign_action(call: &CallExpression) -> Option<Action> {
    let arg = call.arguments.first()?.as_expression()?;
    let obj = match arg {
        Expression::ObjectExpression(o) => o,
        _ => return None,
    };
    let mut args: Vec<serde_json::Value> = Vec::new();
    for p in obj.properties.iter() {
        let op = match p {
            ObjectPropertyKind::ObjectProperty(op) => op,
            _ => continue,
        };
        let key = property_key_name(&op.key)?;
        let val = match &op.value {
            // Only extract literal values; skip functions/expressions
            expr if is_literal_expression(expr) => expr_to_type_and_value(expr).1,
            _ => continue,
        };
        args.push(serde_json::json!(key));
        args.push(val);
    }
    if args.is_empty() {
        return None;
    }
    Some(Action {
        name: "assign".into(),
        args,
    })
}

fn is_literal_expression(expr: &Expression) -> bool {
    match expr {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => true,
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::UnaryNegation => {
            matches!(&unary.argument, arg if is_literal_expression(arg))
        }
        _ => false,
    }
}
