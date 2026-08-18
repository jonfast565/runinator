//! covers the advertised tool list and the help tool that indexes it.

use super::*;

fn names(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

#[test]
fn the_two_general_tools_come_first() {
    let tools = definitions(Vec::new());
    let names = names(&tools);
    assert_eq!(names[0], HELP_TOOL);
    assert_eq!(names[1], EXEC_TOOL);
}

#[test]
fn the_command_surface_is_advertised_alongside_them() {
    let names = names(&definitions(Vec::new()));
    assert!(names.contains(&"runinator_workflows_apply".to_string()));
    assert!(names.len() > 50, "only {} tools advertised", names.len());
}

// the workflow tools are appended, not merged, so switching them off cannot disturb the rest.
#[test]
fn workflow_tools_are_appended_when_given() {
    let extra = vec![json!({ "name": "deploy_0e1f", "description": "x" })];
    let with = names(&definitions(extra));
    let without = names(&definitions(Vec::new()));
    assert_eq!(with.len(), without.len() + 1);
    assert_eq!(with.last().map(String::as_str), Some("deploy_0e1f"));
}

#[test]
fn the_exec_tool_requires_a_command_and_defaults_to_json() {
    let tools = definitions(Vec::new());
    let exec = tools
        .iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some(EXEC_TOOL))
        .expect("the exec tool is advertised");
    assert_eq!(
        exec.pointer("/inputSchema/required")
            .and_then(Value::as_array),
        Some(&vec![Value::from("command")])
    );
    assert_eq!(
        exec.pointer("/inputSchema/properties/json/default")
            .and_then(Value::as_bool),
        Some(true)
    );
    // the surface reaches every verb, including the ones that delete.
    assert_eq!(
        exec.pointer("/annotations/destructiveHint")
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn help_with_no_argument_lists_the_whole_surface() {
    let result = help(&json!({}));
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(false));
    let listed = result
        .get("structuredContent")
        .and_then(Value::as_array)
        .expect("the listing is structured");
    let commands: Vec<&str> = listed
        .iter()
        .filter_map(|entry| entry.get("command").and_then(Value::as_str))
        .collect();
    assert!(commands.contains(&"workflows apply"));
    assert!(commands.contains(&"runs show"));
}

// the console-local verbs (`:help`, `:use`, `:replay`) only mean something inside a session and have
// no command line to run.
#[test]
fn console_only_verbs_are_not_listed() {
    let result = help(&json!({}));
    let listed = result
        .get("structuredContent")
        .and_then(Value::as_array)
        .expect("the listing is structured");
    let commands: Vec<&str> = listed
        .iter()
        .filter_map(|entry| entry.get("command").and_then(Value::as_str))
        .collect();
    for local in ["sessions", "bindings", "replay", "clear"] {
        assert!(!commands.contains(&local), "{local} should not be listed");
    }
}

// one match gets the long form; a prefix that matched several is a menu.
#[test]
fn help_for_one_command_expands_its_arguments() {
    let result = help(&json!({ "command": "settings set" }));
    let arguments = result
        .pointer("/structuredContent/arguments")
        .and_then(Value::as_array)
        .expect("a single match expands");
    let labels: Vec<&str> = arguments
        .iter()
        .filter_map(|argument| argument.get("argument").and_then(Value::as_str))
        .collect();
    assert!(
        labels.iter().any(|label| label.contains("--kind")),
        "{labels:?}"
    );
}

#[test]
fn help_for_a_prefix_narrows_to_a_menu() {
    let result = help(&json!({ "command": "settings" }));
    let listed = result
        .get("structuredContent")
        .and_then(Value::as_array)
        .expect("a prefix lists");
    assert!(listed.len() > 1);
    assert!(listed.iter().all(|entry| {
        entry
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| command.starts_with("settings"))
    }));
}

// a `:`-prefixed topic is how the console writes it, and the same word should work here.
#[test]
fn a_console_style_topic_is_accepted() {
    let result = help(&json!({ "command": ":runs show" }));
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(false));
    assert_eq!(
        result
            .pointer("/structuredContent/command")
            .and_then(Value::as_str),
        Some("runs show")
    );
}

#[test]
fn an_unknown_topic_is_a_tool_error_that_points_back_at_the_listing() {
    let result = help(&json!({ "command": "nonsense" }));
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
    let text = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(text.contains(HELP_TOOL), "{text}");
}
