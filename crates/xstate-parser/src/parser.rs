use std::collections::HashMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::assign;
use crate::error::ParseError;
use xstate_ir::{
    Action, ActionDef, ActorDef, ContextField, ContextSchema, Guard, GuardDef, InvokeConfig,
    Machine, SetupConfig, StateNode, StateType, Transition,
};

pub fn parse(source: &str) -> Result<Machine, ParseError> {
    check_nesting_depth(source)?;
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
    check_nesting_depth(source)?;
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

    if !ret.errors.is_empty() {
        return Err(ParseError::OxcParse);
    }

    collect_all_machines(&ret.program)
}

// Issue #29: the recursive-descent parser (and our AST walkers) overflow the
// stack on pathological nesting, so cap bracket depth before parsing.
const MAX_SOURCE_NESTING_DEPTH: usize = 512;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanState {
    Code,
    LineComment,
    BlockComment,
    SingleQuoted,
    DoubleQuoted,
    Template,
}

fn check_nesting_depth(source: &str) -> Result<(), ParseError> {
    let bytes = source.as_bytes();
    let mut depth: usize = 0;
    let mut template_frames: Vec<ScanState> = Vec::new();
    let mut state = ScanState::Code;
    let mut i = 0;

    while i < bytes.len() {
        let byte = bytes[i];
        i += 1;
        match state {
            ScanState::Code => match byte {
                b'/' if bytes.get(i) == Some(&b'/') => {
                    state = ScanState::LineComment;
                    i += 1;
                }
                b'/' if bytes.get(i) == Some(&b'*') => {
                    state = ScanState::BlockComment;
                    i += 1;
                }
                b'\'' => state = ScanState::SingleQuoted,
                b'"' => state = ScanState::DoubleQuoted,
                b'`' => state = ScanState::Template,
                b'{' | b'[' | b'(' => {
                    depth += 1;
                    if depth > MAX_SOURCE_NESTING_DEPTH {
                        return Err(ParseError::NestingTooDeep(MAX_SOURCE_NESTING_DEPTH));
                    }
                }
                b'}' if template_frames.last() == Some(&ScanState::Template) => {
                    template_frames.pop();
                    state = ScanState::Template;
                }
                b'}' | b']' | b')' => depth = depth.saturating_sub(1),
                _ => {}
            },
            ScanState::LineComment => {
                if byte == b'\n' {
                    state = ScanState::Code;
                }
            }
            ScanState::BlockComment => {
                if byte == b'*' && bytes.get(i) == Some(&b'/') {
                    state = ScanState::Code;
                    i += 1;
                }
            }
            ScanState::SingleQuoted => match byte {
                b'\\' => i += 1,
                b'\'' | b'\n' => state = ScanState::Code,
                _ => {}
            },
            ScanState::DoubleQuoted => match byte {
                b'\\' => i += 1,
                b'"' | b'\n' => state = ScanState::Code,
                _ => {}
            },
            ScanState::Template => match byte {
                b'\\' => i += 1,
                b'`' => state = ScanState::Code,
                b'$' if bytes.get(i) == Some(&b'{') => {
                    template_frames.push(ScanState::Template);
                    state = ScanState::Code;
                    i += 1;
                }
                _ => {}
            },
        }
    }

    Ok(())
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
    if let Some((config, setup)) = try_get_machine_config(call) {
        push_machine(config, &setup, machines)?;
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

/// Extract one `createMachine(...)` config into the machines output.
///
/// Lenient only about *missing* properties (an anonymous machine gets a
/// synthesized id and its first state as initial, matching XState's default
/// behaviour). Constructs that are *present* but unrepresentable — inline
/// guards/actions, non-literal scalars — are typed errors that propagate to
/// the caller (issue #46): a silent drop would return a different machine
/// than the one authored.
fn push_machine(
    config: &ObjectExpression,
    setup: &SetupConfig,
    out: &mut Vec<Machine>,
) -> Result<(), ParseError> {
    let id = require_string_prop(config, "id", "machine 'id'")?
        .unwrap_or_else(|| format!("machine-{}", out.len()));

    let states = match config.get_object_prop_value("states") {
        Some(Expression::ObjectExpression(states_obj)) => {
            extract_states_object(states_obj, &setup.delays)?
        }
        Some(_) => {
            return Err(ParseError::InvalidMachine(
                "'states' must be an object mapping state names to state nodes".to_string(),
            ));
        }
        None => Vec::new(),
    };

    // A root-parallel machine has no single initial state; leave the initial
    // empty so callers can detect and skip it (the IR cannot express a
    // parallel root, and neither runtime can run one faithfully).
    let root_type = match config.get_object_prop_value("type") {
        Some(expr) => {
            let s = extract_string(expr).ok_or_else(|| {
                ParseError::UnsupportedFeature(
                    "machine 'type' is present but not a plain string literal".to_string(),
                )
            })?;
            Some(parse_state_type(&s)?)
        }
        None => None,
    };
    let initial = if root_type == Some(StateType::Parallel) {
        require_string_prop(config, "initial", "machine 'initial'")?.unwrap_or_default()
    } else {
        require_string_prop(config, "initial", "machine 'initial'")?
            .or_else(|| states.first().map(|s| s.name.clone()))
            .unwrap_or_default()
    };

    let context = match config.get_object_prop_value("context") {
        Some(Expression::ObjectExpression(ctx_obj)) => extract_context_schema(ctx_obj)?,
        Some(_) => {
            return Err(ParseError::UnsupportedFeature(
                "'context' must be an object literal".to_string(),
            ));
        }
        None => ContextSchema { fields: vec![] },
    };

    let setup_config = if setup.is_empty() {
        None
    } else {
        Some(setup.clone())
    };

    let mut machine = Machine {
        id,
        initial,
        context,
        states,
        setup: setup_config,
    };
    infer_state_types(&mut machine.states);
    out.push(machine);
    Ok(())
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
            SearchResult::Found(config, setup) => {
                return extract_machine_from_config(config, &setup);
            }
            SearchResult::NotFound => {}
        }
    }

    Err(ParseError::InvalidMachine(
        "No createMachine call found".into(),
    ))
}

enum SearchResult<'a> {
    Found(&'a ObjectExpression<'a>, SetupConfig),
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
                        SearchResult::Found(config, setup) => {
                            return SearchResult::Found(config, setup);
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
                        SearchResult::Found(config, setup) => {
                            return SearchResult::Found(config, setup);
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
) -> Option<(&'a ObjectExpression<'a>, SetupConfig)> {
    let call = match expr {
        Expression::CallExpression(call) => call,
        _ => return None,
    };
    try_get_machine_config(call)
}

fn try_get_machine_config<'a>(
    call: &'a CallExpression<'a>,
) -> Option<(&'a ObjectExpression<'a>, SetupConfig)> {
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

    let setup = if let Expression::StaticMemberExpression(member) = &call.callee {
        if let Expression::CallExpression(setup_call) = &member.object {
            if let Expression::Identifier(ident) = &setup_call.callee {
                if ident.name.as_ref() == "setup" {
                    extract_setup_config(setup_call)
                } else {
                    SetupConfig::default()
                }
            } else {
                SetupConfig::default()
            }
        } else {
            SetupConfig::default()
        }
    } else {
        SetupConfig::default()
    };

    Some((config, setup))
}

fn extract_machine_from_config(
    obj: &ObjectExpression,
    setup: &SetupConfig,
) -> Result<Machine, ParseError> {
    let id = require_string_prop(obj, "id", "machine 'id'")?
        .ok_or_else(|| ParseError::InvalidMachine("Machine missing 'id'".into()))?;

    let initial = require_string_prop(obj, "initial", "machine 'initial'")?
        .ok_or_else(|| ParseError::InvalidMachine("Machine missing 'initial'".into()))?;

    let context = match obj.get_object_prop_value("context") {
        Some(Expression::ObjectExpression(ctx_obj)) => extract_context_schema(ctx_obj)?,
        Some(_) => {
            return Err(ParseError::UnsupportedFeature(
                "'context' must be an object literal".to_string(),
            ));
        }
        None => ContextSchema { fields: vec![] },
    };

    let states = match obj.get_object_prop_value("states") {
        Some(Expression::ObjectExpression(states_obj)) => {
            extract_states_object(states_obj, &setup.delays)?
        }
        Some(_) => {
            return Err(ParseError::InvalidMachine(
                "'states' must be an object mapping state names to state nodes".to_string(),
            ));
        }
        None => Vec::new(),
    };

    let setup_config = if setup.is_empty() {
        None
    } else {
        Some(setup.clone())
    };

    Ok(Machine {
        id,
        initial,
        context,
        states,
        setup: setup_config,
    })
}

fn extract_setup_config(call: &CallExpression) -> SetupConfig {
    let setup_arg = match call.arguments.first().and_then(|arg| arg.as_expression()) {
        Some(Expression::ObjectExpression(obj)) => obj,
        _ => return SetupConfig::default(),
    };

    let actors = extract_actors_from_setup(setup_arg);
    let guards = extract_guards_from_setup(setup_arg);
    let actions = extract_actions_from_setup(setup_arg);
    let delays = extract_delays_from_setup(setup_arg);

    SetupConfig {
        actors,
        guards,
        actions,
        delays,
    }
}

fn extract_actors_from_setup(setup_config: &ObjectExpression) -> HashMap<String, ActorDef> {
    let mut actors = HashMap::new();

    let actors_obj = match setup_config.get_object_prop_value("actors") {
        Some(Expression::ObjectExpression(obj)) => obj,
        _ => return actors,
    };

    for prop in actors_obj.properties.iter() {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };

        let key_name = match property_key_name(&p.key) {
            Some(n) => n.to_string(),
            _ => continue,
        };

        // The value is typically a call expression like fromPromise(...), fromCallback(...), etc.
        let actor_def = match &p.value {
            Expression::CallExpression(call) => {
                let callee_name = extract_callee_name(call);
                match callee_name.as_deref() {
                    Some("fromPromise") => ActorDef::FromPromise {
                        src: key_name.clone(),
                    },
                    Some("fromCallback") => ActorDef::FromCallback {
                        src: key_name.clone(),
                    },
                    Some("fromActorLogic") => ActorDef::FromActorLogic {
                        src: key_name.clone(),
                    },
                    Some("fromMachine") => ActorDef::FromMachine {
                        machine_id: key_name.clone(),
                    },
                    _ => ActorDef::FromActorLogic {
                        src: key_name.clone(),
                    },
                }
            }
            _ => ActorDef::FromActorLogic {
                src: key_name.clone(),
            },
        };

        actors.insert(key_name, actor_def);
    }

    actors
}

fn extract_callee_name(call: &CallExpression) -> Option<String> {
    match &call.callee {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(member.property.name.to_string()),
        _ => None,
    }
}

fn extract_guards_from_setup(setup_config: &ObjectExpression) -> HashMap<String, GuardDef> {
    let mut guards = HashMap::new();

    let guards_obj = match setup_config.get_object_prop_value("guards") {
        Some(Expression::ObjectExpression(obj)) => obj,
        _ => return guards,
    };

    for prop in guards_obj.properties.iter() {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };

        let key_name = match property_key_name(&p.key) {
            Some(n) => n.to_string(),
            _ => continue,
        };

        guards.insert(
            key_name.clone(),
            GuardDef {
                name: key_name,
                params: vec![],
            },
        );
    }

    guards
}

fn extract_actions_from_setup(setup_config: &ObjectExpression) -> HashMap<String, ActionDef> {
    let mut actions = HashMap::new();

    let actions_obj = match setup_config.get_object_prop_value("actions") {
        Some(Expression::ObjectExpression(obj)) => obj,
        _ => return actions,
    };

    for prop in actions_obj.properties.iter() {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };

        let key_name = match property_key_name(&p.key) {
            Some(n) => n.to_string(),
            _ => continue,
        };

        actions.insert(
            key_name.clone(),
            ActionDef {
                name: key_name,
                params: vec![],
            },
        );
    }

    actions
}

fn extract_delays_from_setup(setup_config: &ObjectExpression) -> HashMap<String, u64> {
    let mut delays = HashMap::new();

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

/// Fail-closed string-prop extraction: a prop that is *present* must resolve
/// to a plain string literal (or a non-interpolated template literal), or the
/// construct is rejected with a typed error. Absent props return `None`.
///
/// This is the distinction behind issue #46: falling back for a present-but-
/// unresolvable value silently produces a different machine, while an absent
/// prop may legitimately have a default (e.g. the synthetic `machine-N` id).
fn require_string_prop(
    obj: &ObjectExpression,
    key: &str,
    what: &str,
) -> Result<Option<String>, ParseError> {
    match obj.get_object_prop_value(key) {
        None => Ok(None),
        Some(expr) => match extract_string(expr) {
            Some(s) => Ok(Some(s)),
            None => Err(ParseError::UnsupportedFeature(format!(
                "{what} is present but not a plain string literal — use a literal value; \
                 silently substituting a default would produce a different machine"
            ))),
        },
    }
}

/// Convert a plain JSON-literal expression into a `serde_json::Value`,
/// rejecting anything that is not representable as serializable data.
/// Guards params, action args, and context defaults are data: a function
/// body or identifier reference cannot be carried into the IR, so it must
/// be rejected rather than silently nulled.
fn extract_literal_json(expr: &Expression, what: &str) -> Result<serde_json::Value, ParseError> {
    match expr {
        Expression::StringLiteral(s) => Ok(serde_json::Value::String(s.value.to_string())),
        Expression::NumericLiteral(n) => {
            // Keep the existing integral-vs-float serialization semantics.
            let val = n.value;
            if val.fract() == 0.0 && val >= i64::MIN as f64 && val <= i64::MAX as f64 {
                Ok(serde_json::json!(val as i64))
            } else {
                Ok(serde_json::json!(val))
            }
        }
        Expression::BooleanLiteral(b) => Ok(serde_json::Value::Bool(b.value)),
        Expression::NullLiteral(_) => Ok(serde_json::Value::Null),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            Ok(serde_json::Value::String(t.quasis[0].value.raw.to_string()))
        }
        Expression::ObjectExpression(obj) => {
            let mut map = serde_json::Map::new();
            for prop in obj.properties.iter() {
                let ObjectPropertyKind::ObjectProperty(op) = prop else {
                    return Err(ParseError::UnsupportedFeature(format!(
                        "{what} cannot contain spread properties"
                    )));
                };
                let Some(key) = property_key_name(&op.key) else {
                    return Err(ParseError::UnsupportedFeature(format!(
                        "{what} keys must be plain identifiers or string literals"
                    )));
                };
                map.insert(key.to_string(), extract_literal_json(&op.value, what)?);
            }
            Ok(serde_json::Value::Object(map))
        }
        Expression::ArrayExpression(arr) => {
            let mut items = Vec::new();
            for elem in arr.elements.iter() {
                match elem.as_expression() {
                    Some(e) => items.push(extract_literal_json(e, what)?),
                    None => {
                        return Err(ParseError::UnsupportedFeature(format!(
                            "{what} arrays cannot contain empty slots"
                        )));
                    }
                }
            }
            Ok(serde_json::Value::Array(items))
        }
        _ => Err(ParseError::UnsupportedFeature(format!(
            "{what} must be plain JSON literals (strings, numbers, booleans, null, arrays, objects); \
             functions and identifier references are not serializable data"
        ))),
    }
}

/// Derive the IR `type_name` label from a literal JSON value, matching the
/// labels the extractor has always produced.
fn json_type_name(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(_) => "string".into(),
        serde_json::Value::Number(_) => "number".into(),
        serde_json::Value::Bool(_) => "boolean".into(),
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Array(_) => "array".into(),
        serde_json::Value::Object(_) => "object".into(),
    }
}

/// Build the typed rejection for a behavior-bearing expression that cannot be
/// represented in the IR (issue #46): inline functions, identifier references
/// to functions, and calls other than `assign(...)`.
fn unsupported_behavior(kind: &str, expr: &Expression) -> ParseError {
    let detail = match expr {
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
            "inline function expressions cannot be represented in IR".to_string()
        }
        Expression::Identifier(id) => {
            format!(
                "identifier reference '{id}' cannot be resolved at extraction time — \
                 use the action/guard name string instead"
            )
        }
        Expression::CallExpression(call) => {
            let name = extract_callee_name(call).unwrap_or_else(|| "<unknown>".to_string());
            if matches!(name.as_str(), "and" | "or" | "not") {
                format!(
                    "higher-order guard '{name}(...)' is not representable in IR — \
                     declare a named composed guard instead (the GuardRegistry provides \
                     and()/or()/not() combinators)"
                )
            } else {
                format!(
                    "call to '{name}(...)' is not supported here — \
                     only assign(...) calls are accepted as inline actions"
                )
            }
        }
        _ => "expression cannot be represented in IR".to_string(),
    };
    ParseError::UnsupportedFeature(format!("{kind}: {detail}"))
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

/// Fail-closed extraction for `entry`/`exit` action lists (issue #46): only
/// string names and the existing `assign(...)` subset are representable —
/// inline functions and identifier references are typed errors, never
/// silently dropped. (`tags` stays lenient: it is metadata, not behavior.)
fn extract_string_action_list(
    arr: &ArrayExpression,
    what: &str,
) -> Result<Vec<String>, ParseError> {
    let mut out = Vec::new();
    for elem in arr.elements.iter() {
        let Some(expr) = elem.as_expression() else {
            return Err(ParseError::UnsupportedFeature(format!(
                "{what} arrays cannot contain empty slots"
            )));
        };
        match expr {
            Expression::StringLiteral(s) => out.push(s.value.to_string()),
            Expression::CallExpression(call) if is_assign_call(call) => {
                out.push("assign".to_string())
            }
            _ => return Err(unsupported_behavior(what, expr)),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Object property helpers
// ---------------------------------------------------------------------------

pub(crate) fn property_key_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
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

fn extract_context_schema(obj: &ObjectExpression) -> Result<ContextSchema, ParseError> {
    let mut fields = Vec::new();

    for prop in obj.properties.iter() {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            return Err(ParseError::UnsupportedFeature(
                "context cannot contain spread properties".to_string(),
            ));
        };
        let name = match property_key_name(&p.key) {
            Some(n) => n.to_string(),
            None => {
                return Err(ParseError::UnsupportedFeature(
                    "context field keys must be plain identifiers or string literals".to_string(),
                ));
            }
        };

        // Context defaults are carried as serializable data; a function or
        // identifier default cannot be represented and must not silently
        // degrade to null (issue #46).
        let default_value = extract_literal_json(&p.value, &format!("context field '{name}'"))?;
        let type_name = json_type_name(&default_value);

        fields.push(ContextField {
            name,
            type_name,
            default_value,
        });
    }

    Ok(ContextSchema { fields })
}

pub(crate) fn expr_to_type_and_value(expr: &Expression) -> (String, serde_json::Value) {
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
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            return Err(ParseError::UnsupportedFeature(
                "'states' cannot contain spread properties".to_string(),
            ));
        };
        let name = match property_key_name(&p.key) {
            Some(n) => n.to_string(),
            None => {
                return Err(ParseError::UnsupportedFeature(
                    "state keys must be plain identifiers or string literals".to_string(),
                ));
            }
        };

        let Expression::ObjectExpression(state_obj) = &p.value else {
            return Err(ParseError::UnsupportedFeature(format!(
                "state '{name}' must be an object literal — silently skipping it would \
                 leave dangling transitions and produce a different machine"
            )));
        };
        let state = extract_state_node(&name, state_obj, delays)?;
        states.push(state);
    }

    Ok(states)
}

fn extract_state_node(
    name: &str,
    obj: &ObjectExpression,
    delays: &HashMap<String, u64>,
) -> Result<StateNode, ParseError> {
    let description = extract_string_prop(obj, "description");

    let node_type = match obj.get_object_prop_value("type") {
        Some(expr) => {
            let s = extract_string(expr).ok_or_else(|| {
                ParseError::UnsupportedFeature(
                    "state 'type' is present but not a plain string literal".to_string(),
                )
            })?;
            parse_state_type(&s)?
        }
        None => StateType::default(),
    };

    let initial = require_string_prop(obj, "initial", "state 'initial'")?;

    let entry = match obj.get_object_prop_value("entry") {
        Some(Expression::ArrayExpression(arr)) => extract_string_action_list(arr, "entry")?,
        Some(_) => {
            return Err(ParseError::UnsupportedFeature(
                "entry must be an array of action names".to_string(),
            ));
        }
        None => Vec::new(),
    };

    let exit = match obj.get_object_prop_value("exit") {
        Some(Expression::ArrayExpression(arr)) => extract_string_action_list(arr, "exit")?,
        Some(_) => {
            return Err(ParseError::UnsupportedFeature(
                "exit must be an array of action names".to_string(),
            ));
        }
        None => Vec::new(),
    };

    let transitions = match obj.get_object_prop_value("on") {
        Some(Expression::ObjectExpression(on_obj)) => extract_transitions(on_obj)?,
        Some(_) => {
            return Err(ParseError::UnsupportedFeature(
                "'on' must be an object mapping event names to transitions".to_string(),
            ));
        }
        None => Vec::new(),
    };

    // always transitions (eventless) — extract from array
    let always_transitions: Vec<Transition> = match obj.get_object_prop_value("always") {
        Some(Expression::ArrayExpression(arr)) => extract_always_transitions(arr)?,
        Some(_) => {
            return Err(ParseError::UnsupportedFeature(
                "'always' must be an array of transitions".to_string(),
            ));
        }
        None => Vec::new(),
    };

    // after (delayed) transitions
    let after_transitions: Vec<Transition> = match obj.get_object_prop_value("after") {
        Some(Expression::ObjectExpression(after_obj)) => {
            extract_after_transitions(after_obj, delays)?
        }
        Some(_) => {
            return Err(ParseError::UnsupportedFeature(
                "'after' must be an object mapping delays to transitions".to_string(),
            ));
        }
        None => Vec::new(),
    };

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
                    Ok(vec![extract_transition_from_obj("", on_done_obj)?])
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
                                    transitions.push(extract_transition_from_obj("", obj)?);
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

    let children = match obj.get_object_prop_value("states") {
        Some(Expression::ObjectExpression(children_obj)) => {
            extract_states_object(children_obj, delays)?
        }
        Some(_) => {
            return Err(ParseError::UnsupportedFeature(
                "nested 'states' must be an object mapping state names to state nodes".to_string(),
            ));
        }
        None => Vec::new(),
    };

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
        output: None,
    })
}

fn parse_state_type(s: &str) -> Result<StateType, ParseError> {
    match s {
        "atomic" => Ok(StateType::Atomic),
        "compound" => Ok(StateType::Compound),
        "parallel" => Ok(StateType::Parallel),
        "final" => Ok(StateType::Final),
        "history" => Ok(StateType::History),
        "deepHistory" | "deephistory" => Ok(StateType::DeepHistory),
        other => Err(ParseError::UnsupportedFeature(format!(
            "unknown state type '{other}' — expected one of atomic, compound, parallel, \
             final, history, deepHistory; silently coercing it to atomic would produce \
             a different machine"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Transitions
// ---------------------------------------------------------------------------

fn extract_transitions(on_obj: &ObjectExpression) -> Result<Vec<Transition>, ParseError> {
    let mut transitions = Vec::new();

    for prop in on_obj.properties.iter() {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            return Err(ParseError::UnsupportedFeature(
                "'on' cannot contain spread properties".to_string(),
            ));
        };
        let event = match property_key_name(&p.key) {
            Some(n) => n.to_string(),
            None => {
                return Err(ParseError::UnsupportedFeature(
                    "event keys in 'on' must be plain identifiers or string literals".to_string(),
                ));
            }
        };

        let mut event_transitions = extract_transition_value(&event, &p.value)?;
        transitions.append(&mut event_transitions);
    }

    Ok(transitions)
}

fn extract_transition_value(
    event: &str,
    value: &Expression,
) -> Result<Vec<Transition>, ParseError> {
    match value {
        // Simple string target: `on: { TOGGLE: 'active' }`
        _ if extract_string(value).is_some() => Ok(vec![Transition {
            event: event.to_string(),
            target: extract_string(value),
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
                    let t = extract_single_transition(event, elem_expr)?;
                    transitions.push(t);
                } else {
                    return Err(ParseError::UnsupportedFeature(format!(
                        "event '{event}': transition arrays cannot contain empty slots"
                    )));
                }
            }
            Ok(transitions)
        }
        // Object: `on: { SUBMIT: { target: 'processing', actions: ['log'] } }`
        Expression::ObjectExpression(obj) => Ok(vec![extract_transition_from_obj(event, obj)?]),
        _ => Err(unsupported_behavior(
            &format!("transition for event '{event}'"),
            value,
        )),
    }
}

fn extract_single_transition(event: &str, expr: &Expression) -> Result<Transition, ParseError> {
    match expr {
        // String inside array: `[{ target: 'a' }, 'b']` -> simple target
        _ if extract_string(expr).is_some() => Ok(Transition {
            event: event.to_string(),
            target: extract_string(expr),
            guard: None,
            delay_ms: None,
            targets: vec![],
            actions: vec![],
            description: None,
        }),
        Expression::ObjectExpression(obj) => extract_transition_from_obj(event, obj),
        _ => Err(unsupported_behavior(
            &format!("transition for event '{event}'"),
            expr,
        )),
    }
}

fn extract_transition_from_obj(
    event: &str,
    obj: &ObjectExpression,
) -> Result<Transition, ParseError> {
    let target = require_string_prop(obj, "target", "transition 'target'")?;

    let guard = match obj.get_object_prop_value("guard") {
        Some(g) => extract_guard_expr(g)?,
        None => None,
    };

    let actions = obj
        .get_object_prop_value("actions")
        .map(extract_actions_expr)
        .transpose()?
        .unwrap_or_default();

    let description = extract_string_prop(obj, "description");

    Ok(Transition {
        event: event.to_string(),
        target,
        guard,
        actions,
        description,
        delay_ms: None,
        targets: vec![],
    })
}

fn extract_always_transitions(arr: &ArrayExpression) -> Result<Vec<Transition>, ParseError> {
    let mut transitions = Vec::new();

    for elem in arr.elements.iter() {
        if let Some(elem_expr) = elem.as_expression() {
            match elem_expr {
                Expression::ObjectExpression(obj) => {
                    let t = extract_transition_from_obj("", obj)?;
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
                _ => {
                    return Err(unsupported_behavior("'always' transition", elem_expr));
                }
            }
        } else {
            return Err(ParseError::UnsupportedFeature(
                "'always' arrays cannot contain empty slots".to_string(),
            ));
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
            return Err(ParseError::UnsupportedFeature(
                "'after' cannot contain spread properties".to_string(),
            ));
        };

        let key_name = property_key_name(&obj_prop.key);
        if key_name.is_none() {
            return Err(ParseError::UnsupportedFeature(
                "'after' keys must be plain identifiers, string literals, or numeric delay \
                 values"
                    .to_string(),
            ));
        }

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
    let src = require_string_prop(invoke_obj, "src", "invoke 'src'")?
        .ok_or_else(|| ParseError::InvalidMachine("invoke missing 'src'".into()))?;

    let id = require_string_prop(invoke_obj, "id", "invoke 'id'")?;

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
                    )?;
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
                    )?;
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

fn extract_guard_expr(expr: &Expression) -> Result<Option<Guard>, ParseError> {
    // Simple string guard: `guard: 'isValid'`
    if let Some(s) = extract_string(expr) {
        return Ok(Some(Guard {
            name: s,
            params: None,
        }));
    }

    match expr {
        // Object guard: `guard: { type: 'isValid', params: {...} }`
        // `params` are serializable data and are carried into the IR — the
        // docs-recommended form must not lose its payload (issue #46).
        Expression::ObjectExpression(obj) => {
            let name = extract_string_prop(obj, "type")
                .or_else(|| extract_string_prop(obj, "name"))
                .ok_or_else(|| {
                    ParseError::UnsupportedFeature(
                        "guard object must declare a string 'type' or 'name'".to_string(),
                    )
                })?;
            let params = match obj.get_object_prop_value("params") {
                Some(p) => Some(extract_literal_json(p, "guard params")?),
                None => None,
            };
            Ok(Some(Guard { name, params }))
        }
        // Arrow functions, identifier references, and calls such as
        // and()/or()/not() cannot be represented in the IR — reject instead
        // of silently dropping the guard (which would allow the transition
        // the author intended to block).
        _ => Err(unsupported_behavior("guard", expr)),
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

fn extract_actions_expr(expr: &Expression) -> Result<Vec<Action>, ParseError> {
    match expr {
        Expression::StringLiteral(s) => Ok(vec![Action {
            name: s.value.to_string(),
            args: vec![],
        }]),
        Expression::ArrayExpression(arr) => {
            let mut actions = Vec::new();
            for elem in arr.elements.iter() {
                actions.push(extract_action_from_element(elem)?);
            }
            Ok(actions)
        }
        Expression::CallExpression(call) => {
            if is_assign_call(call) {
                Ok(vec![require_assign_action(call)?])
            } else {
                Err(unsupported_behavior("action", expr))
            }
        }
        _ => Err(unsupported_behavior("action", expr)),
    }
}

fn extract_action_from_element(elem: &ArrayExpressionElement) -> Result<Action, ParseError> {
    let Some(expr) = elem.as_expression() else {
        return Err(ParseError::UnsupportedFeature(
            "action arrays cannot contain empty slots".to_string(),
        ));
    };
    match expr {
        Expression::StringLiteral(s) => Ok(Action {
            name: s.value.to_string(),
            args: vec![],
        }),
        Expression::ObjectExpression(obj) => {
            let Some(name) = extract_string_prop(obj, "type") else {
                return Err(ParseError::UnsupportedFeature(
                    "action object must declare a string 'type'".to_string(),
                ));
            };
            // Remaining props are carried as serializable args.
            let mut args: Vec<serde_json::Value> = Vec::new();
            for p in obj.properties.iter() {
                let ObjectPropertyKind::ObjectProperty(op) = p else {
                    return Err(ParseError::UnsupportedFeature(
                        "action objects cannot contain spread properties".to_string(),
                    ));
                };
                let Some(key) = property_key_name(&op.key) else {
                    return Err(ParseError::UnsupportedFeature(
                        "action object keys must be plain identifiers or string literals"
                            .to_string(),
                    ));
                };
                if key == "type" || key == "name" {
                    continue;
                }
                args.push(extract_literal_json(
                    &op.value,
                    &format!("action '{name}' argument '{key}'"),
                )?);
            }
            Ok(Action { name, args })
        }
        Expression::CallExpression(call) if is_assign_call(call) => require_assign_action(call),
        _ => Err(unsupported_behavior("action", expr)),
    }
}

/// `assign(...)` is the only accepted inline call. A present call whose
/// argument is not an object literal is a typed error, not a silent no-op.
fn require_assign_action(call: &CallExpression) -> Result<Action, ParseError> {
    extract_assign_action(call)?.ok_or_else(|| {
        ParseError::UnsupportedFeature(
            "assign(...) must be called with a single object literal argument".to_string(),
        )
    })
}

fn is_assign_call(call: &CallExpression) -> bool {
    match &call.callee {
        // assign(...)
        callee if callee.is_specific_id("assign") => true,
        _ => false,
    }
}

fn extract_assign_action(call: &CallExpression) -> Result<Option<Action>, ParseError> {
    let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) else {
        return Ok(None);
    };
    let obj = match arg {
        Expression::ObjectExpression(o) => o,
        _ => return Ok(None),
    };
    let mut static_args: Vec<serde_json::Value> = Vec::new();
    let mut ops: Vec<serde_json::Value> = Vec::new();
    let mut has_dynamic = false;
    for p in obj.properties.iter() {
        let op = match p {
            ObjectPropertyKind::ObjectProperty(op) => op,
            _ => continue,
        };
        let Some(key) = property_key_name(&op.key) else {
            continue;
        };
        let key = key.to_string();
        match &op.value {
            expr if is_literal_expression(expr) => {
                let val = expr_to_type_and_value(expr).1;
                static_args.push(serde_json::json!(key));
                static_args.push(val.clone());
                ops.push(serde_json::json!({
                    "op": "set",
                    "field": key,
                    "value": val,
                }));
            }
            Expression::ArrowFunctionExpression(arrow) => {
                has_dynamic = true;
                ops.push(assign::lower_updater(&key, arrow)?);
            }
            _ => {}
        }
    }
    if !has_dynamic {
        if static_args.is_empty() {
            return Ok(None);
        }
        return Ok(Some(Action {
            name: "assign".into(),
            args: static_args,
        }));
    }
    if ops.is_empty() {
        return Ok(None);
    }
    Ok(Some(Action {
        name: "assign".into(),
        args: vec![serde_json::json!({ "$assign_ops": ops })],
    }))
}

pub(crate) fn is_literal_expression(expr: &Expression) -> bool {
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
