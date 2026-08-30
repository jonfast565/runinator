//! covers what exec refuses to run and how it prepares a line, without a web service on the other
//! end. running a command is the integration path; deciding whether to is this one.

use super::*;

fn tokens(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_string()).collect()
}

// a verb that never returns would hang the tool call rather than answer it.
#[test]
fn the_interactive_and_watching_verbs_are_refused() {
    for refused in [
        vec!["console"],
        vec!["mcp"],
        vec!["login"],
        vec!["logout"],
        vec!["workflows", "dev"],
        vec!["runs", "watch"],
        vec!["pipelines", "run-watch"],
    ] {
        assert!(
            blocked_for(&tokens(&refused)).is_some(),
            "{refused:?} should be refused"
        );
    }
}

#[test]
fn everything_else_is_allowed() {
    for allowed in [
        vec!["workflows", "apply"],
        vec!["workflows", "list"],
        vec!["runs", "show"],
        vec!["settings", "get"],
    ] {
        assert!(
            blocked_for(&tokens(&allowed)).is_none(),
            "{allowed:?} should be allowed"
        );
    }
}

// longest-first is what lets `runs watch` be refused while `runs` stays open.
#[test]
fn a_longer_blocked_path_does_not_shadow_a_shorter_open_one() {
    assert!(blocked_for(&tokens(&["runs", "list"])).is_none());
    assert!(blocked_for(&tokens(&["runs", "watch", "abc"])).is_some());
    assert_eq!(
        blocked_for(&tokens(&["workflows", "dev"])).map(blocked_name),
        Some("workflows dev".to_string())
    );
}

// the refusal is matched on words, so a flag before the verb cannot slip one through.
#[test]
fn flags_do_not_count_as_command_words() {
    assert!(blocked_for(&tokens(&["--json", "console"])).is_none());
    assert!(blocked_for(&tokens(&["console", "--plain"])).is_some());
}

#[test]
fn a_refusal_says_what_to_do_instead() {
    let blocked = blocked_for(&tokens(&["console"])).expect("console is refused");
    assert!(
        blocked.reason.contains("command line"),
        "the alternative should be named: {}",
        blocked.reason
    );
}

#[test]
fn the_json_flag_is_appended_once() {
    assert_eq!(
        with_json_flag(tokens(&["runs", "list"]), true),
        tokens(&["runs", "list", "--json"])
    );
    // a caller that wrote it already should not get it twice — clap accepts the repeat, but the
    // line stops reading as what was asked for.
    assert_eq!(
        with_json_flag(tokens(&["runs", "list", "--json"]), true),
        tokens(&["runs", "list", "--json"])
    );
}

#[test]
fn json_off_leaves_the_line_alone() {
    assert_eq!(
        with_json_flag(tokens(&["runs", "list"]), false),
        tokens(&["runs", "list"])
    );
}
