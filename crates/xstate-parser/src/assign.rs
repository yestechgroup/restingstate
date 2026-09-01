use oxc_ast::ast::*;

use crate::error::ParseError;
use crate::parser::{expr_to_type_and_value, is_literal_expression, property_key_name};

pub(super) fn lower_updater(
    field: &str,
    arrow: &ArrowFunctionExpression,
) -> Result<serde_json::Value, ParseError> {
    validate_params(field, &arrow.params)?;
    if !arrow.expression {
        return Err(reject(
            field,
            "updater must be an expression body, e.g. ({ context }) => context.count + 1; block bodies are not supported",
        ));
    }
    let stmt = arrow
        .body
        .statements
        .first()
        .ok_or_else(|| reject(field, "updater has an empty body"))?;
    let expr = match stmt {
        Statement::ExpressionStatement(es) => strip_parens(&es.expression),
        _ => return Err(reject(field, "updater body must be a single expression")),
    };
    lower_expression(field, expr)
}

fn reject(field: &str, reason: &str) -> ParseError {
    ParseError::UnsupportedAssign(format!("assign updater for '{field}': {reason}"))
}

fn strip_parens<'a>(mut expr: &'a Expression<'a>) -> &'a Expression<'a> {
    while let Expression::ParenthesizedExpression(paren) = expr {
        expr = &paren.expression;
    }
    expr
}

fn validate_params(field: &str, params: &FormalParameters) -> Result<(), ParseError> {
    if params.items.is_empty() {
        return Ok(());
    }
    if params.items.len() > 1 || params.rest.is_some() {
        return Err(reject(
            field,
            "updater must take no arguments or a single ({ context, event }) destructuring",
        ));
    }
    let pattern = &params.items[0].pattern;
    let BindingPatternKind::ObjectPattern(obj) = &pattern.kind else {
        return Err(reject(
            field,
            "updater parameter must be a destructuring ({ context, event })",
        ));
    };
    if obj.rest.is_some() {
        return Err(reject(
            field,
            "rest elements in the updater parameter are not supported",
        ));
    }
    for prop in &obj.properties {
        if prop.computed {
            return Err(reject(
                field,
                "computed keys in the updater parameter are not supported",
            ));
        }
        if !prop.shorthand {
            return Err(reject(
                field,
                "aliased bindings are not supported; use ({ context, event }) verbatim",
            ));
        }
        let name = property_key_name(&prop.key)
            .ok_or_else(|| reject(field, "unsupported parameter binding"))?;
        if name != "context" && name != "event" {
            return Err(reject(
                field,
                &format!(
                    "updater may only bind 'context' and 'event', found '{name}' (expected the ({{ context, event }}) form)"
                ),
            ));
        }
    }
    Ok(())
}

fn lower_expression(field: &str, expr: &Expression) -> Result<serde_json::Value, ParseError> {
    let expr = strip_parens(expr);
    match expr {
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => Err(reject_call(field, &call.callee)),
            ChainElement::TSNonNullExpression(non_null) => {
                lower_expression(field, &non_null.expression)
            }
            ChainElement::StaticMemberExpression(member) => {
                let (root, path) = member_root_path(field, member)?;
                Ok(set_from_op(field, root, &path))
            }
            ChainElement::ComputedMemberExpression(_) => Err(reject_computed(field)),
            ChainElement::PrivateFieldExpression(_) => Err(reject_private(field)),
        },
        Expression::BinaryExpression(bin) => lower_binary(field, bin),
        Expression::CallExpression(call) => Err(reject_call(field, &call.callee)),
        Expression::TemplateLiteral(_) => Err(reject(
            field,
            "string templates/concatenation are not supported in assign updaters",
        )),
        Expression::StaticMemberExpression(member) => {
            let (root, path) = member_root_path(field, member)?;
            Ok(set_from_op(field, root, &path))
        }
        expr if is_literal_expression(expr) => {
            let value = expr_to_type_and_value(expr).1;
            Ok(serde_json::json!({
                "op": "set",
                "field": field,
                "value": value,
            }))
        }
        Expression::Identifier(_) => Err(reject(
            field,
            "bare 'context'/'event' identifiers are not allowed; access a property, e.g. context.count",
        )),
        Expression::ComputedMemberExpression(_) => Err(reject_computed(field)),
        Expression::PrivateFieldExpression(_) => Err(reject_private(field)),
        _ => Err(reject(
            field,
            "unsupported expression form (allowed: context/event paths, field +/- numeric literal, plain literals)",
        )),
    }
}

fn set_from_op(field: &str, root: &str, path: &str) -> serde_json::Value {
    serde_json::json!({
        "op": "set",
        "field": field,
        "from": format!("{root}.{path}"),
    })
}

fn reject_computed(field: &str) -> ParseError {
    reject(
        field,
        "computed member access (obj[expr]) is not supported in assign updaters",
    )
}

fn reject_private(field: &str) -> ParseError {
    reject(
        field,
        "private field access is not supported in assign updaters",
    )
}

fn reject_call(field: &str, callee: &Expression) -> ParseError {
    match callee {
        Expression::Identifier(ident) => reject(
            field,
            &format!(
                "unknown identifier '{name}' (only 'context' and 'event' are available in assign updaters)",
                name = ident.name.as_ref()
            ),
        ),
        Expression::StaticMemberExpression(member) => reject(
            field,
            &format!(
                "method call '.{prop}()' is not supported in assign updaters",
                prop = member.property.name.as_ref()
            ),
        ),
        _ => reject(field, "function calls are not supported in assign updaters"),
    }
}

fn check_root(field: &str, root: &str, segments: &[&str]) -> Result<(), ParseError> {
    if root != "context" && root != "event" {
        return Err(reject(
            field,
            &format!(
                "unknown identifier '{root}' (only 'context' and 'event' are available in assign updaters)"
            ),
        ));
    }
    if segments.is_empty() {
        return Err(reject(
            field,
            &format!("'{root}' must be followed by a property path, e.g. {root}.field"),
        ));
    }
    Ok(())
}

fn member_root_path<'a>(
    field: &str,
    member: &'a StaticMemberExpression<'a>,
) -> Result<(&'a str, String), ParseError> {
    let mut segments: Vec<&str> = vec![member.property.name.as_ref()];
    let mut current = strip_parens(&member.object);
    loop {
        match current {
            Expression::StaticMemberExpression(m) => {
                segments.push(m.property.name.as_ref());
                current = strip_parens(&m.object);
            }
            Expression::ComputedMemberExpression(_) => return Err(reject_computed(field)),
            Expression::PrivateFieldExpression(_) => return Err(reject_private(field)),
            Expression::CallExpression(call) => return Err(reject_call(field, &call.callee)),
            Expression::Identifier(ident) => {
                let root = ident.name.as_ref();
                check_root(field, root, &segments)?;
                segments.reverse();
                return Ok((root, segments.join(".")));
            }
            _ => {
                return Err(reject(field, "unsupported member access in assign updater"));
            }
        }
    }
}

fn source_ref(expr: &Expression) -> Option<(String, String)> {
    let expr = strip_parens(expr);
    let member = match expr {
        Expression::StaticMemberExpression(member) => member,
        _ => return None,
    };
    member_root_path("?", member)
        .ok()
        .map(|(root, path)| (root.to_string(), path))
}

fn numeric_literal_value(expr: &Expression) -> Option<f64> {
    match expr {
        Expression::NumericLiteral(n) => Some(n.value),
        Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::UnaryNegation => {
            match &unary.argument {
                Expression::NumericLiteral(n) => Some(-n.value),
                _ => None,
            }
        }
        _ => None,
    }
}

fn lower_binary(field: &str, bin: &BinaryExpression) -> Result<serde_json::Value, ParseError> {
    let op_name = match bin.operator {
        BinaryOperator::Addition => "add",
        BinaryOperator::Subtraction => "sub",
        other => {
            return Err(reject(
                field,
                &format!(
                    "unsupported binary operator '{}' (only '+' and '-' with a numeric literal are supported)",
                    operator_symbol(other)
                ),
            ));
        }
    };

    let left = strip_parens(&bin.left);
    let right = strip_parens(&bin.right);
    let left_ref = source_ref(left);
    let right_ref = source_ref(right);

    let (base, delta_expr) = match (&left_ref, &right_ref) {
        (Some(_), Some(_)) => {
            return Err(reject(
                field,
                "arithmetic between two context/event fields is not supported; combine a field with a numeric literal",
            ));
        }
        (None, None) => {
            return Err(reject(
                field,
                "arithmetic must combine a context field with a numeric literal, e.g. context.count + 1",
            ));
        }
        (Some(_), None) => (left, right),
        (None, Some(_)) => {
            if bin.operator == BinaryOperator::Subtraction {
                return Err(reject(
                    field,
                    "subtracting a context/event field from a literal is not supported",
                ));
            }
            (right, left)
        }
    };

    let (root, path) = source_ref(base).expect("base must be a source reference");
    if root != "context" {
        return Err(reject(
            field,
            "arithmetic on event fields is not supported; add/sub applies to context fields",
        ));
    }
    if path != field {
        return Err(reject(
            field,
            &format!("arithmetic must read the assigned field '{field}', found '{path}'"),
        ));
    }

    let delta = numeric_literal_value(delta_expr).ok_or_else(|| {
        reject(
            field,
            "the other operand of '+'/'-' must be a numeric literal",
        )
    })?;

    Ok(serde_json::json!({
        "op": op_name,
        "field": field,
        "value": number_value(delta),
    }))
}

fn number_value(v: f64) -> serde_json::Value {
    if v.fract() == 0.0 && v >= i64::MIN as f64 && v <= i64::MAX as f64 {
        serde_json::json!(v as i64)
    } else {
        serde_json::json!(v)
    }
}

fn operator_symbol(op: BinaryOperator) -> &'static str {
    match op {
        BinaryOperator::Addition => "+",
        BinaryOperator::Subtraction => "-",
        BinaryOperator::Multiplication => "*",
        BinaryOperator::Division => "/",
        BinaryOperator::Remainder => "%",
        BinaryOperator::Exponential => "**",
        BinaryOperator::ShiftLeft => "<<",
        BinaryOperator::ShiftRight => ">>",
        BinaryOperator::ShiftRightZeroFill => ">>>",
        BinaryOperator::BitwiseOR => "|",
        BinaryOperator::BitwiseXOR => "^",
        BinaryOperator::BitwiseAnd => "&",
        BinaryOperator::Equality => "==",
        BinaryOperator::Inequality => "!=",
        BinaryOperator::StrictEquality => "===",
        BinaryOperator::StrictInequality => "!==",
        BinaryOperator::LessThan => "<",
        BinaryOperator::LessEqualThan => "<=",
        BinaryOperator::GreaterThan => ">",
        BinaryOperator::GreaterEqualThan => ">=",
        BinaryOperator::In => "in",
        BinaryOperator::Instanceof => "instanceof",
    }
}
