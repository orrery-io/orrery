use super::{ScriptError, ScriptEvaluator, ScriptInput, ScriptOutput};
use serde_json::Value;
use std::collections::HashMap;

pub struct RhaiEvaluator {
    engine: rhai::Engine,
}

impl Default for RhaiEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl RhaiEvaluator {
    pub fn new() -> Self {
        let mut engine = rhai::Engine::new();
        engine.set_max_operations(10_000);
        engine.set_max_call_levels(16);
        engine.set_max_string_size(10_000);
        engine.set_max_array_size(1_000);
        Self { engine }
    }
}

impl ScriptEvaluator for RhaiEvaluator {
    fn language(&self) -> &str {
        "rhai"
    }

    fn eval(&self, input: ScriptInput) -> Result<ScriptOutput, ScriptError> {
        let mut scope = rhai::Scope::new();

        // Load process variables into Rhai scope
        for (key, value) in input.variables {
            push_json_to_scope(&mut scope, key, value);
        }

        // Compile and run
        let ast = self.engine.compile(input.script).map_err(|e| ScriptError {
            message: e.to_string(),
            line: e.position().line(),
        })?;

        let result: rhai::Dynamic =
            self.engine
                .eval_ast_with_scope(&mut scope, &ast)
                .map_err(|e| ScriptError {
                    message: e.to_string(),
                    line: e.position().line(),
                })?;

        // Convert return value
        let return_value = dynamic_to_json(&result);

        // Collect modified/new variables from scope
        let mut modified_variables = HashMap::new();
        for (name, _constant, value) in scope.iter() {
            let json_val = dynamic_to_json(&value);
            if let Some(json_val) = json_val {
                let changed = input
                    .variables
                    .get(name)
                    .map(|orig| *orig != json_val)
                    .unwrap_or(true); // new variable
                if changed {
                    modified_variables.insert(name.to_string(), json_val);
                }
            }
        }

        Ok(ScriptOutput {
            return_value,
            modified_variables,
        })
    }
}

fn push_json_to_scope(scope: &mut rhai::Scope, key: &str, value: &Value) {
    match value {
        Value::Null => {
            scope.push(key.to_string(), ());
        }
        Value::Bool(b) => {
            scope.push(key.to_string(), *b);
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                scope.push(key.to_string(), i);
            } else if let Some(f) = n.as_f64() {
                scope.push(key.to_string(), f);
            }
        }
        Value::String(s) => {
            scope.push(key.to_string(), s.clone());
        }
        Value::Array(arr) => {
            let rhai_arr: rhai::Array = arr.iter().map(json_to_dynamic).collect();
            scope.push(key.to_string(), rhai_arr);
        }
        Value::Object(map) => {
            let mut rhai_map = rhai::Map::new();
            for (k, v) in map {
                rhai_map.insert(k.clone().into(), json_to_dynamic(v));
            }
            scope.push(key.to_string(), rhai_map);
        }
    }
}

fn json_to_dynamic(value: &Value) -> rhai::Dynamic {
    match value {
        Value::Null => rhai::Dynamic::UNIT,
        Value::Bool(b) => rhai::Dynamic::from(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                rhai::Dynamic::from(i)
            } else {
                rhai::Dynamic::from(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => rhai::Dynamic::from(s.clone()),
        Value::Array(arr) => {
            let rhai_arr: rhai::Array = arr.iter().map(json_to_dynamic).collect();
            rhai::Dynamic::from(rhai_arr)
        }
        Value::Object(map) => {
            let mut rhai_map = rhai::Map::new();
            for (k, v) in map {
                rhai_map.insert(k.clone().into(), json_to_dynamic(v));
            }
            rhai::Dynamic::from(rhai_map)
        }
    }
}

fn dynamic_to_json(value: &rhai::Dynamic) -> Option<Value> {
    if value.is_unit() {
        return None;
    }
    if value.is_bool() {
        return Some(Value::Bool(value.as_bool().unwrap()));
    }
    if value.is_int() {
        return Some(Value::Number(value.as_int().unwrap().into()));
    }
    if value.is_float() {
        return value
            .as_float()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number);
    }
    if value.is_string() {
        return Some(Value::String(value.clone().into_string().unwrap()));
    }
    if value.is_array() {
        let arr = value
            .clone()
            .into_typed_array::<rhai::Dynamic>()
            .unwrap_or_default();
        let json_arr: Vec<Value> = arr.iter().filter_map(dynamic_to_json).collect();
        return Some(Value::Array(json_arr));
    }
    if value.is_map() {
        let map = value.clone().cast::<rhai::Map>();
        let json_map: serde_json::Map<String, Value> = map
            .iter()
            .filter_map(|(k, v)| dynamic_to_json(v).map(|jv| (k.to_string(), jv)))
            .collect();
        return Some(Value::Object(json_map));
    }
    None
}
