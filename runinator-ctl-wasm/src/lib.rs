//! Browser bindings for the portable `runinatorctl` command language.

use runinator_ctl_core::console;
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Catalog,
    Parse { line: String },
    Complete { line: String },
    IsSubmittable { source: String },
}

/// Execute one small command-language operation through a stable JSON boundary. Keeping the ABI
/// here means TypeScript never needs to understand clap internals and adding an operation does not
/// multiply generated binding types.
#[wasm_bindgen]
pub fn invoke(request: &str) -> String {
    let response = serde_json::from_str::<Request>(request)
        .map_err(|error| error.to_string())
        .and_then(dispatch);
    match response {
        Ok(value) => json!({ "ok": true, "value": value }).to_string(),
        Err(error) => json!({ "ok": false, "error": error }).to_string(),
    }
}

fn dispatch(request: Request) -> Result<Value, String> {
    match request {
        Request::Catalog => serde_json::to_value(console::command_specs()),
        Request::Parse { line } => serde_json::to_value(console::parse_line(&line)?),
        Request::Complete { line } => serde_json::to_value(console::complete(&line)),
        Request::IsSubmittable { source } => Ok(Value::Bool(console::is_submittable(&source))),
    }
    .map_err(|error| error.to_string())
}
