use std::{fs, path::Path};

use runinator_models::value::{Map, Value};

use crate::commands::{Result, err};

pub fn load_json_file(path: &Path) -> Result<Value> {
    let data = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

pub fn load_object(json_file: Option<&Path>, params: &[String]) -> Result<Value> {
    let mut value = match json_file {
        Some(path) => load_json_file(path)?,
        None => Value::Object(Map::new()),
    };

    if params.is_empty() {
        return Ok(value);
    }

    let Some(object) = value.as_object_mut() else {
        return Err(err(
            "--param can only be used when the JSON payload is an object",
        ));
    };

    for param in params {
        let Some((key, raw_value)) = param.split_once('=') else {
            return Err(err(format!(
                "parameter '{param}' must use KEY=VALUE syntax"
            )));
        };
        if key.trim().is_empty() {
            return Err(err("parameter key cannot be empty"));
        }
        object.insert(key.into(), parse_value(raw_value));
    }

    Ok(value)
}

fn parse_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.into()))
}

#[cfg(test)]
#[path = "params_tests.rs"]
mod tests;
