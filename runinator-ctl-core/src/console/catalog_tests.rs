//! covers the derived command catalog: what it lists, how it spells a call, and what it knows
//! about the values an argument accepts.

use super::*;

fn path(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_string()).collect()
}

fn entry(name: &str) -> &'static CommandEntry {
    catalog()
        .iter()
        .find(|entry| entry.name() == name)
        .unwrap_or_else(|| panic!("'{name}' is in the catalog"))
}

#[test]
fn lists_console_verbs_and_every_command_line_leaf() {
    let names: Vec<String> = catalog().iter().map(CommandEntry::name).collect();

    assert!(names.contains(&"help".to_string()));
    assert!(names.contains(&"run workflow".to_string()));
    assert!(names.contains(&"workflows list".to_string()));
    assert!(names.contains(&"replicas list".to_string()));
    // a parent is not itself callable, so it is not listed on its own.
    assert!(!names.contains(&"workflows".to_string()));
}

#[test]
fn a_console_verb_is_marked_as_one() {
    assert!(entry("sessions").console_local);
    assert!(!entry("runs list").console_local);
}

#[test]
fn usage_names_the_arguments_a_command_takes() {
    assert_eq!(entry("runs show").usage, "runs show <ID>");
    assert!(entry("workflows run").usage.contains("[--param KEY=VALUE]"));
    // a required positional is angle-bracketed and an optional one is not.
    assert!(entry("workflows export").usage.contains("[WORKFLOW_ID]"));
}

#[test]
fn a_short_closed_set_is_spelled_out_in_the_usage() {
    assert!(entry("rexrap check").usage.contains("strict|permissive"));
    assert!(entry("runs timeline").usage.contains("table|graph|json"));
}

#[test]
fn knows_the_values_a_flag_accepts() {
    assert_eq!(
        flag_values(&path(&["replicas", "list"]), "status"),
        vec!["live", "stale", "offline"]
    );
    assert!(flag_values(&path(&["nodes", "scale"]), "kind").contains(&"worker".to_string()));
    // a free-form flag has no closed set to offer.
    assert!(flag_values(&path(&["runs", "list"]), "status").is_empty());
    assert_eq!(
        flag_values(&path(&["runs", "timeline"]), "format"),
        vec!["table", "graph", "json"]
    );
}

#[test]
fn knows_which_flags_consume_the_next_word() {
    assert!(flag_takes_value(&path(&["runs", "list"]), "status"));
    assert!(!flag_takes_value(&path(&["runs", "list"]), "open"));
    // a global flag is found from any depth.
    assert!(!flag_takes_value(&path(&["runs", "list"]), "json"));
}

#[test]
fn describes_a_positional_by_name_and_help() {
    let (label, help) = positional_hint(&path(&["workflows", "show"]), 0).expect("a positional");

    assert_eq!(label, "<WORKFLOW>");
    assert!(help.is_empty() || help.len() < 200);
    assert!(positional_hint(&path(&["workflows", "list"]), 0).is_none());
}

#[test]
fn matches_the_longest_console_verb_path() {
    let matched = match_meta(&path(&["run", "workflow", "daily"])).expect("a console verb");

    assert_eq!(matched.path, ["run", "workflow"]);
    assert!(match_meta(&path(&["workflows", "list"])).is_none());
}

#[test]
fn every_console_verb_is_explained() {
    for meta in META_COMMANDS {
        assert!(!meta.summary.is_empty(), "{:?} has no summary", meta.path);
        assert!(
            meta.usage.starts_with(meta.path.join(" ").as_str()),
            "{:?} usage does not start with its path",
            meta.path
        );
    }
}
