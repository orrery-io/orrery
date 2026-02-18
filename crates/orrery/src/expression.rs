use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

/// Evaluate a BPMN condition expression against a set of process variables.
/// Returns `false` on any parse error or undefined variable (with a warning log).
///
/// Supported syntax:
/// - Comparisons: `${var} == value`, `${var} != value`, `${var} > value`, etc.
/// - Boolean: `expr && expr`, `expr || expr`, `!expr`
/// - Parentheses: `(expr)`
/// - Variable references: `${varName}` or bare `varName`
/// - Literals: numbers, strings (single-quoted), `true`, `false`
pub fn eval(expr: &str, variables: &HashMap<String, Value>) -> bool {
    eval_or(expr.trim(), variables)
}

/// Evaluate a BPMN correlation key expression and convert the result to a String.
///
/// Supported expression forms (in evaluation order):
/// - FEEL: `= varName` → look up `variables[varName]`
/// - JUEL: `${varName}` → look up `variables[varName]`
/// - Bare variable name: look up `variables[expr]`
///
/// Returns `None` if the variable is not found or the value is not a scalar.
pub fn eval_to_string(expr: &str, variables: &HashMap<String, Value>) -> Option<String> {
    let expr = expr.trim();

    // FEEL prefix: "= varName"
    let key = if let Some(rest) = expr.strip_prefix("= ") {
        rest.trim()
    } else if expr.starts_with("${") && expr.ends_with('}') {
        // JUEL: ${varName}
        &expr[2..expr.len() - 1]
    } else {
        expr
    };

    let val = variables.get(key)?;
    value_to_string(val)
}

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

// --- Precedence layers ---

fn eval_or(expr: &str, vars: &HashMap<String, Value>) -> bool {
    // Split on top-level `||` (not inside parens or ${})
    if let Some((i, _)) = find_top_level(expr, "||").into_iter().next() {
        let left = &expr[..i];
        let right = &expr[i + 2..];
        return eval_or(left.trim(), vars) || eval_or(right.trim(), vars);
    }
    eval_and(expr, vars)
}

fn eval_and(expr: &str, vars: &HashMap<String, Value>) -> bool {
    // Split on top-level `&&`
    if let Some((i, _)) = find_top_level(expr, "&&").into_iter().next() {
        let left = &expr[..i];
        let right = &expr[i + 2..];
        return eval_and(left.trim(), vars) && eval_and(right.trim(), vars);
    }
    eval_atom(expr, vars)
}

fn eval_atom(expr: &str, vars: &HashMap<String, Value>) -> bool {
    let expr = expr.trim();

    // NOT
    if let Some(inner) = expr.strip_prefix('!') {
        return !eval_atom(inner, vars);
    }

    // Parentheses
    if expr.starts_with('(') && expr.ends_with(')') {
        return eval_or(&expr[1..expr.len() - 1], vars);
    }

    // Comparison: find the operator
    eval_comparison(expr, vars)
}

// --- Comparison ---

fn eval_comparison(expr: &str, vars: &HashMap<String, Value>) -> bool {
    // Try operators longest-first to avoid misparse (>= before >)
    const OPS: &[&str] = &["==", "!=", ">=", "<=", ">", "<"];

    for op in OPS {
        if let Some(pos) = find_operator(expr, op) {
            let lhs = expr[..pos].trim();
            let rhs = expr[pos + op.len()..].trim();
            let left = resolve_value(lhs, vars);
            let right = parse_literal(rhs);
            return compare(&left, op, &right);
        }
    }

    // No operator — treat bare variable as a truthiness check
    let key = strip_delimiters(expr);
    if let Some(val) = vars.get(key) {
        value_is_truthy(val)
    } else {
        warn!("expression: variable '{}' not found", key);
        false
    }
}

fn value_is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn compare(left: &Option<Value>, op: &str, right: &Option<Value>) -> bool {
    let (Some(l), Some(r)) = (left, right) else {
        return false;
    };
    match op {
        "==" => values_equal(l, r),
        "!=" => !values_equal(l, r),
        ">" => numeric_compare(l, r) == Some(std::cmp::Ordering::Greater),
        "<" => numeric_compare(l, r) == Some(std::cmp::Ordering::Less),
        ">=" => matches!(
            numeric_compare(l, r),
            Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal)
        ),
        "<=" => matches!(
            numeric_compare(l, r),
            Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal)
        ),
        _ => false,
    }
}

fn values_equal(l: &Value, r: &Value) -> bool {
    // Direct JSON equality first
    if l == r {
        return true;
    }
    // Type coercion for comparisons across representations
    match (l, r) {
        // Numeric coercion: int 2 == float 2.0
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        // String vs number coercion: "42" == 42
        (Value::String(s), Value::Number(n)) => s.parse::<f64>().ok() == n.as_f64(),
        (Value::Number(n), Value::String(s)) => n.as_f64() == s.parse::<f64>().ok(),
        _ => false,
    }
}

fn numeric_compare(l: &Value, r: &Value) -> Option<std::cmp::Ordering> {
    let lf = to_f64(l)?;
    let rf = to_f64(r)?;
    lf.partial_cmp(&rf)
}

fn to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

// --- Value resolution ---

/// Resolve an expression token to a Value: variable lookup or literal parse.
fn resolve_value(token: &str, vars: &HashMap<String, Value>) -> Option<Value> {
    let token = token.trim();
    if token.starts_with("${") && token.ends_with('}') {
        let key = &token[2..token.len() - 1];
        if let Some(v) = vars.get(key) {
            Some(v.clone())
        } else {
            warn!("expression: variable '{}' not found", key);
            None
        }
    } else {
        // Could be a bare variable name or a literal
        if let Some(v) = vars.get(token) {
            Some(v.clone())
        } else {
            Some(parse_literal_raw(token))
        }
    }
}

/// Parse a literal token into a serde_json Value.
fn parse_literal(token: &str) -> Option<Value> {
    Some(parse_literal_raw(token))
}

fn parse_literal_raw(token: &str) -> Value {
    match token {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" => Value::Null,
        s if s.starts_with('\'') && s.ends_with('\'') => {
            Value::String(s[1..s.len() - 1].to_string())
        }
        s if s.starts_with('"') && s.ends_with('"') => Value::String(s[1..s.len() - 1].to_string()),
        s => {
            if let Ok(n) = s.parse::<i64>() {
                Value::Number(n.into())
            } else if let Ok(n) = s.parse::<f64>() {
                Value::Number(
                    serde_json::Number::from_f64(n).unwrap_or(serde_json::Number::from(0)),
                )
            } else {
                Value::String(s.to_string())
            }
        }
    }
}

fn strip_delimiters(s: &str) -> &str {
    if s.starts_with("${") && s.ends_with('}') {
        &s[2..s.len() - 1]
    } else {
        s
    }
}

// --- Tokenisation helpers ---

/// Find all top-level (not inside parens or ${}) occurrences of `needle` in `expr`.
/// Returns positions from RIGHT to LEFT so callers can split on the first one found
/// (which gives left-to-right evaluation semantics).
fn find_top_level(expr: &str, needle: &str) -> Vec<(usize, usize)> {
    let bytes = expr.as_bytes();
    let nlen = needle.len();
    let mut depth = 0i32;
    let mut in_bpmn_var = false;
    let mut results = Vec::new();
    let mut i = 0;

    while i <= expr.len().saturating_sub(nlen) {
        // Track ${} depth
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            in_bpmn_var = true;
            i += 2;
            continue;
        }
        if in_bpmn_var && bytes[i] == b'}' {
            in_bpmn_var = false;
            i += 1;
            continue;
        }
        if in_bpmn_var {
            i += 1;
            continue;
        }

        // Track paren depth
        if bytes[i] == b'(' {
            depth += 1;
            i += 1;
            continue;
        }
        if bytes[i] == b')' {
            depth -= 1;
            i += 1;
            continue;
        }

        if depth == 0 && expr[i..].starts_with(needle) {
            results.push((i, i + nlen));
        }
        i += 1;
    }

    // Return rightmost first so callers split on the LAST top-level operator
    results.reverse();
    results
}

/// Find the position of `op` in `expr` at top level.
/// Returns the rightmost position.
fn find_operator(expr: &str, op: &str) -> Option<usize> {
    find_top_level(expr, op)
        .into_iter()
        .next()
        .map(|(pos, _)| pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn eval_to_string_feel_prefix() {
        let v = vars(&[("orderId", json!("ord-123"))]);
        assert_eq!(eval_to_string("= orderId", &v).as_deref(), Some("ord-123"));
    }

    #[test]
    fn eval_to_string_juel_syntax() {
        let v = vars(&[("orderId", json!("ord-456"))]);
        assert_eq!(eval_to_string("${orderId}", &v).as_deref(), Some("ord-456"));
    }

    #[test]
    fn eval_to_string_bare_variable() {
        let v = vars(&[("orderId", json!("ord-789"))]);
        assert_eq!(eval_to_string("orderId", &v).as_deref(), Some("ord-789"));
    }

    #[test]
    fn eval_to_string_number_variable() {
        let v = vars(&[("amount", json!(42))]);
        assert_eq!(eval_to_string("= amount", &v).as_deref(), Some("42"));
    }

    #[test]
    fn eval_to_string_missing_variable_returns_none() {
        let v = vars(&[]);
        assert_eq!(eval_to_string("= orderId", &v), None);
    }

    #[test]
    fn eval_to_string_complex_value_returns_none() {
        let v = vars(&[("obj", json!({"key": "value"}))]);
        assert_eq!(eval_to_string("= obj", &v), None);
    }

    // ── Numeric coercion tests ────────────────────────────────────────────

    #[test]
    fn int_literal_equals_int_variable() {
        // json!(2) stores as Number(PosInt(2)); literal "2" should parse as i64
        let v = vars(&[("count", json!(2))]);
        assert!(eval("${count} == 2", &v));
    }

    #[test]
    fn int_variable_equals_float_literal() {
        // json!(2) (int) vs literal "2.0" (float) — should coerce via as_f64()
        let v = vars(&[("count", json!(2))]);
        assert!(eval("${count} == 2.0", &v));
    }

    #[test]
    fn float_variable_equals_int_literal() {
        // json!(2.0) (float) vs literal "2" (parsed as i64)
        let v = vars(&[("count", json!(2.0))]);
        assert!(eval("${count} == 2", &v));
    }

    #[test]
    fn string_number_coercion_in_equality() {
        // String "42" compared against number 42
        let v = vars(&[("val", json!("42"))]);
        assert!(eval("${val} == 42", &v));
    }

    #[test]
    fn number_string_coercion_in_equality() {
        // Number 42 compared against string literal '42'
        let v = vars(&[("val", json!(42))]);
        assert!(eval("${val} == '42'", &v));
    }

    #[test]
    fn int_not_equal_different_int() {
        let v = vars(&[("task", json!(2))]);
        assert!(!eval("${task} == 3", &v));
    }

    #[test]
    fn int_greater_than() {
        let v = vars(&[("task", json!(3))]);
        assert!(eval("${task} > 2", &v));
        assert!(!eval("${task} > 3", &v));
    }

    #[test]
    fn int_less_than_or_equal() {
        let v = vars(&[("task", json!(2))]);
        assert!(eval("${task} <= 2", &v));
        assert!(eval("${task} <= 3", &v));
        assert!(!eval("${task} <= 1", &v));
    }

    #[test]
    fn parse_literal_prefers_integer() {
        // Verify that "42" parses as integer (i64), not float
        let v = parse_literal_raw("42");
        assert!(v.is_number());
        // Should be representable as i64
        assert_eq!(v.as_i64(), Some(42));
    }

    #[test]
    fn parse_literal_negative_integer() {
        let v = parse_literal_raw("-5");
        assert_eq!(v.as_i64(), Some(-5));
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn parse_literal_float() {
        let v = parse_literal_raw("3.14");
        assert!(v.as_f64().is_some());
        assert!((v.as_f64().unwrap() - 3.14).abs() < 0.001);
    }

    #[test]
    fn parse_literal_non_numeric_becomes_string() {
        let v = parse_literal_raw("hello");
        assert_eq!(v, json!("hello"));
    }
}
