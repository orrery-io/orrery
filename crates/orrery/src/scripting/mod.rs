mod rhai_eval;

use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug)]
pub struct ScriptInput<'a> {
    pub script: &'a str,
    pub variables: &'a HashMap<String, Value>,
}

#[derive(Debug)]
pub struct ScriptOutput {
    pub return_value: Option<Value>,
    pub modified_variables: HashMap<String, Value>,
}

#[derive(Debug)]
pub struct ScriptError {
    pub message: String,
    pub line: Option<usize>,
}

pub trait ScriptEvaluator: Send + Sync {
    fn language(&self) -> &str;
    fn eval(&self, input: ScriptInput) -> Result<ScriptOutput, ScriptError>;
}

pub use rhai_eval::RhaiEvaluator;
