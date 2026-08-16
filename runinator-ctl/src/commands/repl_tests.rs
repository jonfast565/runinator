//! covers the console repl's line parsing: tokenization and the clap surface it feeds.

use super::*;

use crate::cli::{RunCommands, WorkflowCommands};

fn tokens(line: &str) -> Vec<String> {
    tokenize(line).expect("line tokenizes")
}

#[test]
fn splits_on_whitespace() {
    assert_eq!(tokens("workflows list"), vec!["workflows", "list"]);
    assert_eq!(tokens("  runs   show  7 "), vec!["runs", "show", "7"]);
}

#[test]
fn keeps_quoted_json_as_one_argument() {
    assert_eq!(
        tokens(r#"settings set aws key '{"a": 1}'"#),
        vec!["settings", "set", "aws", "key", r#"{"a": 1}"#]
    );
}

#[test]
fn keeps_an_empty_quoted_argument() {
    assert_eq!(
        tokens(r#"runs rename 1 """#),
        vec!["runs", "rename", "1", ""]
    );
}

#[test]
fn single_quotes_keep_backslashes() {
    assert_eq!(
        tokens(r"wdl check 'C:\packs\a.wdl'"),
        vec!["wdl", "check", r"C:\packs\a.wdl"]
    );
}

#[test]
fn escapes_a_space_outside_quotes() {
    assert_eq!(
        tokens(r"wdl check my\ pack.wdl"),
        vec!["wdl", "check", "my pack.wdl"]
    );
}

#[test]
fn rejects_an_unterminated_quote() {
    assert!(tokenize(r#"settings set a b "unclosed"#).is_err());
}

#[test]
fn parses_a_command_line_command() {
    let parsed = parse(&tokens("workflows list")).expect("parses");
    assert!(matches!(
        parsed.command,
        Commands::Workflows {
            command: WorkflowCommands::List
        }
    ));
    assert!(!parsed.json);
}

#[test]
fn accepts_json_after_the_subcommand() {
    let parsed = parse(&tokens("runs list --open --json")).expect("parses");
    assert!(parsed.json);
    assert!(matches!(
        parsed.command,
        Commands::Runs {
            command: RunCommands::List { open: true, .. }
        }
    ));
}

#[test]
fn reports_an_unknown_verb() {
    let error = parse(&tokens("nonsense")).expect_err("unknown verb is rejected");
    assert!(error.to_string().contains("nonsense"));
}

#[test]
fn help_lists_console_and_command_line_verbs() {
    let text = help(None).expect("help renders");
    assert!(text.contains(":bindings"));
    assert!(text.contains("workflows"));
    assert!(text.contains("settings"));
}

#[test]
fn help_for_one_command_shows_its_flags() {
    let text = help(Some("runs")).expect("help renders");
    assert!(text.contains("watch"));
}

#[test]
fn completion_sources_come_from_the_clap_tree() {
    let names = command_names();
    assert!(names.contains(&"workflows".to_string()));
    assert!(names.contains(&"agents".to_string()));
    assert!(subcommand_names("runs").contains(&"cancel".to_string()));
    assert!(flag_names(&["runs".into(), "list".into()]).contains(&"--status".to_string()));
}

#[test]
fn submits_a_finished_line_and_waits_for_an_open_one() {
    assert!(is_submittable("1 + 2"));
    assert!(is_submittable(":workflows list"));
    assert!(!is_submittable("workflow \"x\" v1 {"));
    assert!(is_submittable(
        "workflow \"x\" v1 {\n  yield { value: 1 }\n}"
    ));
    assert!(!is_submittable("\"unfinished"));
    assert!(!is_submittable("1 + \\"));
    assert!(!is_submittable("   "));
}

#[test]
fn completes_only_command_lines() {
    assert!(complete("1 + ").options.is_empty());

    let offered = complete(":work").options;
    assert_eq!(offered, vec!["workflows".to_string()]);

    let subcommands = complete(":workflows ").options;
    assert!(subcommands.contains(&"apply".to_string()));

    let flags = complete(":runs list --").options;
    assert!(flags.contains(&"--status".to_string()));
}

#[test]
fn a_completion_replaces_only_the_word_being_typed() {
    let line = ":workflows appl";
    assert_eq!(&line[complete(line).start..], "appl");
}
