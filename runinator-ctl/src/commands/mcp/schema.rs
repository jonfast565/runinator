//! one tool per `runinatorctl` command, derived from the clap tree.
//!
//! the whole command surface is advertised, so a model sees `workflows apply` as a tool with named,
//! typed, documented arguments rather than as a command line it has to compose. every part of that
//! is *read* from clap — the name from the command path, the description from its `about`, the
//! properties from its `Arg`s, their closed sets from `possible_values`, their defaults from
//! `default_value` — so a verb added to `Commands` is a tool the day it is added, and a flag that
//! changes shape changes shape here too. a hand-written schema per command would be ninety copies
//! nobody updates.
//!
//! a call goes back out the same door: `command_line` turns the arguments object into the argv the
//! command line would have produced, and `exec` runs it through the same parser and dispatch as
//! everything else. there is one execution path, not two.

use clap::{Arg, ArgAction, Command};
use runinator_models::json;
use runinator_models::value::Value;

use super::exec;
use super::protocol::object_schema;
use crate::commands::catalog::{self, CommandEntry};

/// the prefix every generated tool name carries, so runinator's tools are one visible group in a
/// client that has several servers connected.
const PREFIX: &str = "runinator_";

/// where an argument goes on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Form {
    /// a bare value, in declaration order.
    Positional,
    /// `--name`, carrying the long flag.
    Flag(String),
}

/// what an argument accepts, which is what decides both its json type and how it is written back
/// out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// a flag that is present or absent.
    Boolean,
    /// one value.
    Text,
    /// a repeatable value.
    List,
}

/// what one value of an argument is, for the json type it gets.
///
/// clap parses `--limit` into an `i64`, and a schema that called it a string would have a strict
/// client quoting the number — or refusing the call. the parser is the only place the rust type
/// survives to runtime, so it is read from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scalar {
    Text,
    Integer,
    Number,
    /// a UUID, which is a string with a format worth naming.
    Uuid,
}

impl Scalar {
    fn json_type(self) -> &'static str {
        match self {
            Scalar::Integer => "integer",
            Scalar::Number => "number",
            Scalar::Text | Scalar::Uuid => "string",
        }
    }

    /// what clap's value parser says the argument is.
    ///
    /// Read the parser's rendering. A derived `Uuid` or `i64` field appears there by type;
    /// everything else stays a string, matching command-line input.
    fn of(argument: &Arg) -> Self {
        let parser = format!("{:?}", argument.get_value_parser());
        if parser.contains("uuid") {
            return Scalar::Uuid;
        }
        if ["i64", "u64", "i32", "u32", "i16", "u16", "usize", "isize"]
            .iter()
            .any(|name| parser.contains(name))
        {
            return Scalar::Integer;
        }
        match parser.contains("f64") || parser.contains("f32") {
            true => Scalar::Number,
            false => Scalar::Text,
        }
    }
}

/// one argument of one command, in the two shapes it has to be known in: a json property and a
/// command-line word.
#[derive(Debug, Clone)]
pub(crate) struct ToolArgument {
    /// the json property name, which is clap's argument id.
    pub key: String,
    pub form: Form,
    pub kind: Kind,
    /// what one value of it is, for the json type.
    pub scalar: Scalar,
    pub required: bool,
    pub description: String,
    /// the closed set of values, when clap knows one.
    pub values: Vec<String>,
    pub default: Option<String>,
}

/// one `runinatorctl` command, as a tool.
#[derive(Debug, Clone)]
pub(crate) struct CommandTool {
    /// the words that select the command, e.g. `["workflows", "apply"]`.
    pub path: Vec<String>,
    pub name: String,
    pub description: String,
    pub arguments: Vec<ToolArgument>,
}

/// every command that can be called over MCP, as a tool.
///
/// built once: the clap tree is walked for each command's arguments, which is not free, and
/// `tools/list` asks for the whole set.
pub(crate) fn command_tools() -> &'static [CommandTool] {
    static TOOLS: std::sync::OnceLock<Vec<CommandTool>> = std::sync::OnceLock::new();
    TOOLS.get_or_init(|| {
        catalog::catalog()
            .iter()
            .filter(|entry| !entry.console_local)
            // Do not advertise a verb that cannot run over MCP. The
            // list is exec's, so the two answers cannot disagree.
            .filter(|entry| exec::blocked_for(&entry.path).is_none())
            .filter_map(build)
            .collect()
    })
}

/// the tool a name selects.
pub(crate) fn find(name: &str) -> Option<&'static CommandTool> {
    command_tools().iter().find(|tool| tool.name == name)
}

/// the MCP tool definitions, one per command.
pub(crate) fn definitions() -> Vec<Value> {
    command_tools().iter().map(definition).collect()
}

fn definition(tool: &CommandTool) -> Value {
    let properties: Vec<(&str, Value)> = tool
        .arguments
        .iter()
        .map(|argument| (argument.key.as_str(), property(argument)))
        .collect();
    let required: Vec<&str> = tool
        .arguments
        .iter()
        .filter(|argument| argument.required)
        .map(|argument| argument.key.as_str())
        .collect();
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": object_schema(&properties, &required),
        "annotations": annotations(tool),
    })
}

fn property(argument: &ToolArgument) -> Value {
    let mut schema = runinator_models::value::Map::new();
    let scalar = argument.scalar.json_type();
    match argument.kind {
        Kind::Boolean => {
            schema.insert("type".into(), Value::from("boolean"));
        }
        Kind::Text => {
            schema.insert("type".into(), Value::from(scalar));
            if argument.scalar == Scalar::Uuid {
                schema.insert("format".into(), Value::from("uuid"));
            }
        }
        Kind::List => {
            schema.insert("type".into(), Value::from("array"));
            schema.insert("items".into(), json!({ "type": scalar }));
        }
    }
    if !argument.description.is_empty() {
        schema.insert(
            "description".into(),
            Value::from(argument.description.clone()),
        );
    }
    if !argument.values.is_empty() {
        let values: Vec<Value> = argument
            .values
            .iter()
            .map(|value| Value::from(value.clone()))
            .collect();
        // an array argument's closed set constrains each item, not the array.
        match argument.kind {
            Kind::List => {
                schema.insert("items".into(), json!({ "type": scalar, "enum": values }));
            }
            _ => {
                schema.insert("enum".into(), Value::Array(values));
            }
        }
    }
    if let Some(default) = &argument.default {
        // a default typed as a string under an `"type": "integer"` property is a schema that
        // contradicts itself, and a strict client validates the default too.
        let default = match argument.kind {
            Kind::Boolean => Value::from(default == "true"),
            _ => match argument.scalar {
                Scalar::Integer | Scalar::Number => serde_json::from_str::<Value>(default)
                    .unwrap_or_else(|_| Value::from(default.clone())),
                _ => Value::from(default.clone()),
            },
        };
        schema.insert("default".into(), default);
    }
    Value::Object(schema)
}

/// the behaviour hints for a command, read off the verb it ends in.
///
/// these are hints, not enforcement — the point is that a client asking for confirmation before a
/// destructive call gets a useful answer rather than the same conservative one for `runs list` and
/// `workflows delete`.
fn annotations(tool: &CommandTool) -> Value {
    let verb = tool.path.last().map(String::as_str).unwrap_or_default();
    let read_only = matches!(
        verb,
        "list"
            | "show"
            | "get"
            | "ids"
            | "logs"
            | "chunks"
            | "status"
            | "diagnostics"
            | "samples"
            | "history"
            | "validate"
            | "test"
            | "check"
            | "diff"
            | "export"
            | "format"
            | "complete"
            | "hover"
            | "runs"
            | "providers"
            | "tree"
    );
    let destructive = matches!(
        verb,
        "delete" | "remove" | "cancel" | "drain" | "restart" | "stop" | "revoke"
    );
    json!({
        "readOnlyHint": read_only,
        "destructiveHint": destructive,
        "idempotentHint": read_only,
        "openWorldHint": true,
    })
}

fn build(entry: &CommandEntry) -> Option<CommandTool> {
    let command = catalog::find(&entry.path)?;
    Some(CommandTool {
        name: tool_name(&entry.path),
        description: describe_command(&command, entry),
        arguments: arguments(&command),
        path: entry.path.clone(),
    })
}

/// a command's path as a tool name: `workflows apply` becomes `runinator_workflows_apply`.
fn tool_name(path: &[String]) -> String {
    let body = path
        .iter()
        .map(|word| word.replace('-', "_"))
        .collect::<Vec<_>>()
        .join("_");
    format!("{PREFIX}{body}")
}

/// what the command is, for the model.
///
/// the long help is preferred over the short one because it is the paragraph the author wrote for a
/// reader who does not already know what the verb does — which is exactly this reader. the call
/// shape is appended so the arguments below have something to hang on.
fn describe_command(command: &Command, entry: &CommandEntry) -> String {
    let mut prose = command
        .get_long_about()
        .or_else(|| command.get_about())
        .map(|about| collapse(&about.to_string()))
        .filter(|about| !about.is_empty())
        .unwrap_or_else(|| entry.summary.clone());
    // clap help is written as a sentence but not always punctuated as one, and the two would run
    // together into a third sentence that says neither thing.
    if !prose.is_empty() && !prose.ends_with(['.', '!', '?', ':']) {
        prose.push('.');
    }
    format!(
        "{prose} Runs `runinatorctl {}` and returns what it printed, as json where the command has \
         a json form.",
        entry.name()
    )
    .trim_start()
    .to_string()
}

/// the arguments a command takes, in declaration order.
fn arguments(command: &Command) -> Vec<ToolArgument> {
    command
        .get_arguments()
        .filter(|argument| catalog::listed(argument))
        .map(argument)
        .collect()
}

fn argument(source: &Arg) -> ToolArgument {
    let kind = match source.get_action() {
        ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count => Kind::Boolean,
        ArgAction::Append => Kind::List,
        _ => Kind::Text,
    };
    // a positional that is not a flag has no long name to write; everything else is written as one.
    let form = match source.get_long() {
        Some(long) => Form::Flag(long.to_string()),
        None => Form::Positional,
    };
    ToolArgument {
        key: source.get_id().as_str().to_string(),
        form,
        kind,
        scalar: Scalar::of(source),
        required: source.is_required_set(),
        description: describe_argument(source),
        values: match catalog::takes_value(source) {
            true => catalog::possible_values(source),
            // a boolean flag reports `true, false` as its values, which says nothing.
            false => Vec::new(),
        },
        default: source
            .get_default_values()
            .first()
            .map(|value| value.to_string_lossy().to_string()),
    }
}

// clap help is written as prose wrapped over several lines. a tool description wants all of it, so
// this only unwraps the lines — unlike `catalog`'s `one_line`, which is cutting a table cell down
// to its first sentence.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// what an argument is.
///
/// the long help is the paragraph the author wrote for someone who does not already know the flag,
/// which is this reader. an argument carrying neither falls back to what `:help` says about it —
/// its closed set or its default — because that beats an empty description.
fn describe_argument(source: &Arg) -> String {
    source
        .get_long_help()
        .or_else(|| source.get_help())
        .map(|help| collapse(&help.to_string()))
        .unwrap_or_else(|| catalog::describe(source))
}

/// the command line a tool call means.
///
/// the result is argv, not a string, so nothing has to be quoted and then unquoted again: a json
/// payload passed as an argument value arrives at clap exactly as it was written.
pub(crate) fn command_line(tool: &CommandTool, arguments: &Value) -> Result<Vec<String>, String> {
    if let Some(unknown) = unknown_argument(tool, arguments) {
        return Err(format!(
            "'{unknown}' is not an argument of `{}`. its arguments are: {}",
            tool.path.join(" "),
            argument_names(tool)
        ));
    }

    let mut positionals: Vec<(String, Vec<String>)> = Vec::new();
    let mut flags: Vec<String> = Vec::new();
    for argument in &tool.arguments {
        let values = values_for(argument, arguments)?;
        if values.is_empty() {
            if argument.required {
                return Err(format!("missing required argument '{}'", argument.key));
            }
            if matches!(argument.form, Form::Positional) {
                positionals.push((argument.key.clone(), Vec::new()));
            }
            continue;
        }
        match &argument.form {
            Form::Positional => positionals.push((argument.key.clone(), values)),
            Form::Flag(long) => {
                for value in values {
                    flags.push(format!("--{long}"));
                    // a boolean flag is the flag; anything else carries its value after it.
                    if argument.kind != Kind::Boolean {
                        flags.push(value);
                    }
                }
            }
        }
    }

    // positionals are matched by position, so a gap cannot be expressed: leaving out the first of
    // two would silently make the second one the first.
    let mut line = tool.path.clone();
    let mut skipped: Option<String> = None;
    for (key, values) in positionals {
        if values.is_empty() {
            skipped.get_or_insert(key);
            continue;
        }
        if let Some(missing) = &skipped {
            return Err(format!(
                "'{key}' comes after '{missing}' on the command line, so '{missing}' has to be given too"
            ));
        }
        line.extend(values);
    }
    line.extend(flags);
    Ok(line)
}

/// the first property that is not an argument of this command.
///
/// silently dropping one would run a different command than the model asked for — `workflows delete`
/// with a misspelled `--yes` is the case that matters.
fn unknown_argument(tool: &CommandTool, arguments: &Value) -> Option<String> {
    let object = arguments.as_object()?;
    object
        .keys()
        .find(|key| {
            key.as_str() != "json"
                && !tool
                    .arguments
                    .iter()
                    .any(|argument| &argument.key == key.as_str())
        })
        .cloned()
}

fn argument_names(tool: &CommandTool) -> String {
    match tool.arguments.is_empty() {
        true => "none — it takes no arguments".to_string(),
        false => tool
            .arguments
            .iter()
            .map(|argument| argument.key.clone())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// the words one argument contributes, empty when it was not given.
fn values_for(argument: &ToolArgument, arguments: &Value) -> Result<Vec<String>, String> {
    let Some(value) = arguments.get(&argument.key) else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    match argument.kind {
        // `false` is how a client says "not set"; writing `--flag` for it would set it.
        Kind::Boolean => match value.as_bool() {
            Some(true) => Ok(vec![String::new()]),
            Some(false) => Ok(Vec::new()),
            None => Err(format!("argument '{}' must be true or false", argument.key)),
        },
        Kind::Text => Ok(vec![scalar(value, &argument.key)?]),
        Kind::List => match value {
            Value::Array(items) => items
                .iter()
                .map(|item| scalar(item, &argument.key))
                .collect(),
            single => Ok(vec![scalar(single, &argument.key)?]),
        },
    }
}

/// one argument value as the word the shell would have carried.
///
/// an object or array is written back as compact json rather than refused: several commands take a
/// json payload as one argument, and a client that sent it as json rather than as a string meant
/// the same thing.
fn scalar(value: &Value, key: &str) -> Result<String, String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string(value).map_err(|failure| failure.to_string())
        }
        Value::Null => Err(format!("argument '{key}' has no value")),
    }
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
