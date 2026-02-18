use orrery::scripting::{RhaiEvaluator, ScriptEvaluator, ScriptInput};
use serde_json::json;
use std::collections::HashMap;

#[test]
fn rhai_simple_arithmetic() {
    let eval = RhaiEvaluator::new();
    let mut vars = HashMap::new();
    vars.insert("a".to_string(), json!(10));
    vars.insert("b".to_string(), json!(20));
    let result = eval
        .eval(ScriptInput {
            script: "a + b",
            variables: &vars,
        })
        .unwrap();
    assert_eq!(result.return_value, Some(json!(30)));
}

#[test]
fn rhai_result_with_string() {
    let eval = RhaiEvaluator::new();
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), json!("world"));
    let result = eval
        .eval(ScriptInput {
            script: r#"  "hello " + name  "#,
            variables: &vars,
        })
        .unwrap();
    assert_eq!(result.return_value, Some(json!("hello world")));
}

#[test]
fn rhai_modified_variables() {
    let eval = RhaiEvaluator::new();
    let mut vars = HashMap::new();
    vars.insert("x".to_string(), json!(5));
    let result = eval
        .eval(ScriptInput {
            script: "let y = x * 2; y",
            variables: &vars,
        })
        .unwrap();
    assert_eq!(result.return_value, Some(json!(10)));
    assert_eq!(result.modified_variables.get("y"), Some(&json!(10)));
}

#[test]
fn rhai_mutated_existing_variable() {
    let eval = RhaiEvaluator::new();
    let mut vars = HashMap::new();
    vars.insert("count".to_string(), json!(1));
    let result = eval
        .eval(ScriptInput {
            script: "count += 10; count",
            variables: &vars,
        })
        .unwrap();
    assert_eq!(result.return_value, Some(json!(11)));
    assert_eq!(result.modified_variables.get("count"), Some(&json!(11)));
}

#[test]
fn rhai_safety_limit_exceeded() {
    let eval = RhaiEvaluator::new();
    let vars = HashMap::new();
    let result = eval.eval(ScriptInput {
        script: "let x = 0; loop { x += 1; }",
        variables: &vars,
    });
    assert!(result.is_err());
}

#[test]
fn rhai_syntax_error() {
    let eval = RhaiEvaluator::new();
    let vars = HashMap::new();
    let result = eval.eval(ScriptInput {
        script: "let x = ;",
        variables: &vars,
    });
    assert!(result.is_err());
}

#[test]
fn rhai_boolean_and_null() {
    let eval = RhaiEvaluator::new();
    let mut vars = HashMap::new();
    vars.insert("flag".to_string(), json!(true));
    let result = eval
        .eval(ScriptInput {
            script: "!flag",
            variables: &vars,
        })
        .unwrap();
    assert_eq!(result.return_value, Some(json!(false)));
}

#[test]
fn rhai_no_return_value() {
    let eval = RhaiEvaluator::new();
    let vars = HashMap::new();
    let result = eval
        .eval(ScriptInput {
            script: "let x = 42;",
            variables: &vars,
        })
        .unwrap();
    // Statement ending with `;` returns () in Rhai — should map to None
    assert_eq!(result.return_value, None);
    assert_eq!(result.modified_variables.get("x"), Some(&json!(42)));
}
