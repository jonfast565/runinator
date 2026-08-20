//! covers the derived per-command tools: their names and schemas, and the command line a call
//! turns back into.

use super::*;
use crate::commands::repl;

fn tool(name: &str) -> &'static CommandTool {
    find(name).unwrap_or_else(|| panic!("no tool named '{name}'"))
}

fn line(name: &str, arguments: Value) -> Vec<String> {
    command_line(tool(name), &arguments).expect("the call should build a command line")
}

// the whole point of deriving: every command is there, without anyone listing them.
#[test]
fn every_command_group_is_advertised() {
    let names: Vec<&str> = command_tools()
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    for expected in [
        "runinator_workflows_apply",
        "runinator_workflows_list",
        "runinator_workflows_run",
        "runinator_runs_show",
        "runinator_settings_set",
        "runinator_functions_publish",
        "runinator_triggers_list",
        "runinator_replicas_list",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
    assert!(
        command_tools().len() > 50,
        "the command surface is much larger than {} tools",
        command_tools().len()
    );
}

// mcp names tools with `^[a-zA-Z0-9_-]{1,64}$`, and a client keys its call table on them.
#[test]
fn tool_names_are_unique_and_legal() {
    let mut seen = std::collections::BTreeSet::new();
    for tool in command_tools() {
        assert!(
            tool.name.len() <= 64
                && tool
                    .name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_'),
            "'{}' is not a usable tool name",
            tool.name
        );
        assert!(
            seen.insert(tool.name.clone()),
            "duplicate tool {}",
            tool.name
        );
    }
}

// advertising a verb exec would refuse would be offering the model a tool that cannot work.
#[test]
fn blocked_verbs_are_not_advertised() {
    for refused in [
        "runinator_console",
        "runinator_mcp",
        "runinator_login",
        "runinator_logout",
        "runinator_workflows_dev",
        "runinator_runs_watch",
    ] {
        assert!(find(refused).is_none(), "{refused} should not be a tool");
    }
}

#[test]
fn a_description_says_what_the_command_does_and_names_it() {
    let described = &tool("runinator_workflows_apply").description;
    assert!(
        described.contains("runinatorctl workflows apply"),
        "{described}"
    );
    assert!(described.len() > 40, "description is too thin: {described}");
}

#[test]
fn every_tool_has_a_description_and_a_closed_schema() {
    for definition in definitions() {
        let name = definition
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("?");
        assert!(
            definition
                .get("description")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty()),
            "{name} has no description"
        );
        assert_eq!(
            definition
                .pointer("/inputSchema/additionalProperties")
                .and_then(Value::as_bool),
            Some(false),
            "{name} accepts unknown arguments"
        );
    }
}

// a required name that is not also a property would make the schema unsatisfiable.
#[test]
fn required_arguments_are_declared_as_properties() {
    for definition in definitions() {
        let name = definition
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let properties = definition
            .pointer("/inputSchema/properties")
            .and_then(Value::as_object)
            .expect("every schema has properties");
        for required in definition
            .pointer("/inputSchema/required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            assert!(
                properties.contains_key(required),
                "{name} requires '{required}', which it does not declare"
            );
        }
    }
}

// the schema is only worth anything if clap agrees with it. a command advertised as needing nothing
// has to parse from its path alone — if it does not, the schema is understating what it needs.
#[test]
fn a_tool_with_no_required_arguments_parses_from_its_path() {
    for tool in command_tools() {
        if tool.arguments.iter().any(|argument| argument.required) {
            continue;
        }
        let line = command_line(tool, &json!({})).expect("nothing is required");
        assert!(
            repl::parse(&line).is_ok(),
            "`{}` is advertised as needing no arguments, but clap disagrees",
            tool.path.join(" ")
        );
    }
}

// and the other direction: what a full call builds has to be a line clap accepts.
#[test]
fn a_built_command_line_parses() {
    let built = line(
        "runinator_settings_set",
        json!({ "scope": "github", "name": "token", "value": "abc", "kind": "secret" }),
    );
    assert_eq!(built[0], "settings");
    assert_eq!(built[1], "set");
    assert!(repl::parse(&built).is_ok(), "clap rejected {built:?}");
}

#[test]
fn positionals_keep_their_declared_order() {
    let built = line(
        "runinator_settings_set",
        json!({ "name": "token", "scope": "github" }),
    );
    let scope = built.iter().position(|word| word == "github").unwrap();
    let name = built.iter().position(|word| word == "token").unwrap();
    assert!(scope < name, "{built:?}");
}

#[test]
fn a_boolean_is_written_as_a_bare_flag_and_omitted_when_false() {
    let on = line(
        "runinator_workflows_run",
        json!({ "workflow": "deploy", "debug": true }),
    );
    assert!(on.contains(&"--debug".to_string()), "{on:?}");
    assert!(repl::parse(&on).is_ok(), "clap rejected {on:?}");

    let off = line(
        "runinator_workflows_run",
        json!({ "workflow": "deploy", "debug": false }),
    );
    assert!(!off.contains(&"--debug".to_string()), "{off:?}");
}

#[test]
fn a_repeatable_flag_is_written_once_per_value() {
    let built = line(
        "runinator_workflows_run",
        json!({ "workflow": "deploy", "params": ["a=1", "b=2"] }),
    );
    assert_eq!(
        built.iter().filter(|word| *word == "--param").count(),
        2,
        "{built:?}"
    );
    assert!(built.contains(&"a=1".to_string()) && built.contains(&"b=2".to_string()));
    assert!(repl::parse(&built).is_ok(), "clap rejected {built:?}");
}

// several commands take a json payload as one argument; a client that sent it as json rather than
// as a string meant the same thing.
#[test]
fn an_object_argument_is_written_as_compact_json() {
    let built = line(
        "runinator_settings_set",
        json!({ "scope": "app", "name": "limits", "value": { "max": 3 }, "kind": "config" }),
    );
    assert!(built.contains(&"{\"max\":3}".to_string()), "{built:?}");
}

#[test]
fn a_number_argument_becomes_its_text() {
    let built = line(
        "runinator_agents_logs",
        json!({ "replica_id": "8f14e45f-ceea-467a-9a2c-8d1e4d1c9b21", "lines": 25 }),
    );
    assert!(built.contains(&"25".to_string()), "{built:?}");
    assert!(repl::parse(&built).is_ok(), "clap rejected {built:?}");
}

#[test]
fn a_missing_required_argument_names_itself() {
    let failure = command_line(
        tool("runinator_settings_set"),
        &json!({ "scope": "github" }),
    )
    .expect_err("name is required");
    assert!(failure.contains("name"), "{failure}");
}

// dropping a misspelled argument would quietly run a different command than the model asked for.
#[test]
fn an_unknown_argument_is_refused_with_the_real_names() {
    let failure = command_line(
        tool("runinator_workflows_run"),
        &json!({ "workflow": "deploy", "debugg": true }),
    )
    .expect_err("'debugg' is not an argument");
    assert!(failure.contains("debugg"), "{failure}");
    assert!(
        failure.contains("debug"),
        "the real names should be offered: {failure}"
    );
}

// `--json` is global and every command takes it, so it is never an unknown argument.
#[test]
fn the_global_json_flag_is_not_an_unknown_argument() {
    assert!(command_line(tool("runinator_runs_list"), &json!({ "json": true })).is_ok());
}

// positionals are matched by position, so a gap cannot be expressed at all: filling the second slot
// while leaving the first empty would silently make the second one the first.
#[test]
fn a_gap_between_positionals_is_refused_rather_than_shifted() {
    let optional = |key: &str| ToolArgument {
        key: key.to_string(),
        form: Form::Positional,
        kind: Kind::Text,
        scalar: Scalar::Text,
        required: false,
        description: String::new(),
        values: Vec::new(),
        default: None,
    };
    let synthetic = CommandTool {
        path: vec!["example".to_string()],
        name: "runinator_example".to_string(),
        description: String::new(),
        arguments: vec![optional("first"), optional("second")],
    };

    assert_eq!(
        command_line(&synthetic, &json!({ "first": "a", "second": "b" })),
        Ok(vec![
            "example".to_string(),
            "a".to_string(),
            "b".to_string()
        ])
    );
    let failure = command_line(&synthetic, &json!({ "second": "b" }))
        .expect_err("'first' comes before 'second'");
    assert!(failure.contains("first"), "{failure}");
}

#[test]
fn a_closed_set_is_advertised_as_an_enum() {
    let definition = definitions()
        .into_iter()
        .find(|definition| {
            definition.get("name").and_then(Value::as_str) == Some("runinator_settings_set")
        })
        .expect("settings set is a tool");
    let kinds = definition
        .pointer("/inputSchema/properties/kind/enum")
        .and_then(Value::as_array)
        .expect("--kind has a closed set");
    assert!(kinds.contains(&Value::from("secret")), "{kinds:?}");
    // a clap default is a default in the schema too, so a client need not guess.
    assert_eq!(
        definition
            .pointer("/inputSchema/properties/kind/default")
            .and_then(Value::as_str),
        Some("secret")
    );
}

// a boolean's `true, false` possible values say nothing, and offering them as an enum would make
// the schema look like a closed string set.
#[test]
fn a_boolean_is_typed_as_a_boolean_without_an_enum() {
    let definition = definitions()
        .into_iter()
        .find(|definition| {
            definition.get("name").and_then(Value::as_str) == Some("runinator_workflows_run")
        })
        .expect("workflows run is a tool");
    assert_eq!(
        definition
            .pointer("/inputSchema/properties/debug/type")
            .and_then(Value::as_str),
        Some("boolean")
    );
    assert!(
        definition
            .pointer("/inputSchema/properties/debug/enum")
            .is_none()
    );
}

#[test]
fn a_repeatable_argument_is_typed_as_an_array_of_strings() {
    let definition = definitions()
        .into_iter()
        .find(|definition| {
            definition.get("name").and_then(Value::as_str) == Some("runinator_workflows_run")
        })
        .expect("workflows run is a tool");
    assert_eq!(
        definition
            .pointer("/inputSchema/properties/params/type")
            .and_then(Value::as_str),
        Some("array")
    );
    assert_eq!(
        definition
            .pointer("/inputSchema/properties/params/items/type")
            .and_then(Value::as_str),
        Some("string")
    );
}

#[test]
fn a_null_argument_reads_as_absent() {
    let built = line(
        "runinator_workflows_run",
        json!({ "workflow": "deploy", "debug": Value::Null }),
    );
    assert!(!built.contains(&"--debug".to_string()), "{built:?}");
}

// a read is not a write, and a client that asks before a destructive call deserves a real answer.
#[test]
fn annotations_separate_reads_from_deletes() {
    let hint = |name: &str, key: &str| {
        definitions()
            .into_iter()
            .find(|definition| definition.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|definition| definition.pointer(&format!("/annotations/{key}")).cloned())
            .and_then(|value| value.as_bool())
    };
    assert_eq!(hint("runinator_workflows_list", "readOnlyHint"), Some(true));
    assert_eq!(
        hint("runinator_workflows_list", "destructiveHint"),
        Some(false)
    );
    assert_eq!(
        hint("runinator_settings_delete", "destructiveHint"),
        Some(true)
    );
    assert_eq!(
        hint("runinator_settings_delete", "readOnlyHint"),
        Some(false)
    );
}

// clap parses `--limit` into an i64 and `<id>` into a Uuid; a schema calling both strings would
// have a strict client quoting the number, or refusing the call.
#[test]
fn numbers_and_uuids_are_typed_as_what_clap_parses_them_into() {
    let definition = definitions()
        .into_iter()
        .find(|definition| {
            definition.get("name").and_then(Value::as_str) == Some("runinator_runs_logs")
        })
        .expect("runs logs is a tool");
    assert_eq!(
        definition
            .pointer("/inputSchema/properties/effect_id/type")
            .and_then(Value::as_str),
        Some("string")
    );
    assert_eq!(
        definition
            .pointer("/inputSchema/properties/effect_id/format")
            .and_then(Value::as_str),
        Some("uuid")
    );
}

#[test]
fn the_value_parser_is_read_for_every_scalar_it_knows() {
    let scalar = |name: &str, key: &str| {
        tool(name)
            .arguments
            .iter()
            .find(|argument| argument.key == key)
            .map(|argument| argument.scalar)
    };
    assert_eq!(
        scalar("runinator_agents_logs", "lines"),
        Some(Scalar::Integer)
    );
    assert_eq!(
        scalar("runinator_runs_show", "id"),
        Some(Scalar::Uuid),
        "a Uuid positional should be recognised"
    );
    assert_eq!(
        scalar("runinator_workflows_run", "workflow"),
        Some(Scalar::Text)
    );
    // a PathBuf is a string on the command line, and json has nothing better to call it.
    assert_eq!(
        scalar("runinator_workflows_run", "json_file"),
        Some(Scalar::Text)
    );
}

// an integer argument still has to survive the trip back out as a command-line word.
#[test]
fn an_integer_argument_round_trips_through_the_parser() {
    let built = line(
        "runinator_agents_logs",
        json!({ "replica_id": "8f14e45f-ceea-467a-9a2c-8d1e4d1c9b21", "lines": 25 }),
    );
    assert!(repl::parse(&built).is_ok(), "clap rejected {built:?}");
}
