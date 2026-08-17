//! covers the console repl's line parsing: tokenization and the clap surface it feeds.

use super::*;

use crate::cli::{RunCommands, WorkflowCommands};

fn tokens(line: &str) -> Vec<String> {
    tokenize(line).expect("line tokenizes")
}

fn scanned(line: &str) -> Vec<Token> {
    scan(line).expect("line scans")
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
fn help_lists_console_and_command_line_verbs_as_a_table() {
    let text = help(None).expect("help renders");

    assert!(text.contains("command"));
    assert!(text.contains("what it does"));
    assert!(text.contains(":bindings"));
    assert!(text.contains(":workflows list"));
    assert!(text.contains(":settings set"));
}

#[test]
fn help_for_a_prefix_lists_every_command_under_it() {
    let text = help(Some("runs")).expect("help renders");

    assert!(text.contains("usage"));
    assert!(text.contains("runs watch"));
    assert!(text.contains("runs cancel"));
}

#[test]
fn help_for_one_command_explains_its_arguments() {
    let text = help(Some("runs list")).expect("help renders");

    assert!(text.contains("argument"));
    assert!(text.contains("--status"));
}

#[test]
fn help_for_an_unknown_topic_suggests_the_nearest_verb() {
    let error = help(Some("wrkflows")).expect_err("no such command");

    assert!(error.to_string().contains("workflows"));
}

#[test]
fn help_for_a_prefix_of_one_command_shows_its_full_call_shape() {
    // `workflow` matches every `workflows …` entry as a prefix, so it lists rather than expands.
    assert!(
        help(Some("workflow"))
            .expect("help renders")
            .contains("workflows apply")
    );
}

#[test]
fn an_unknown_verb_suggests_the_nearest_one() {
    assert!(unknown_command("workflws").contains(":workflows"));
    // something that is not a typo of anything is reported without a guess.
    assert!(!unknown_command("zzzzzzzzzz").contains("did you mean"));
}

#[test]
fn completion_sources_come_from_the_clap_tree() {
    let names = command_names();
    assert!(names.contains(&"workflows".to_string()));
    assert!(names.contains(&"agents".to_string()));
    assert!(names.contains(&"replicas".to_string()));
}

#[test]
fn flags_are_split_from_positionals_in_either_spelling() {
    let parsed = parse_arguments(
        &scanned("daily --param a=1 --param=b=2 --debug --alias production"),
        &["debug"],
    );

    assert_eq!(parsed.args, vec!["daily"]);
    assert_eq!(parsed.flag_list("param"), ["a=1".to_string(), "b=2".into()]);
    assert!(parsed.is_set("debug"));
    assert_eq!(parsed.flag("alias"), Some("production"));
    // a boolean never swallows the word after it.
    assert_eq!(parsed.flag("debug"), None);
}

#[test]
fn a_trailing_flag_reads_as_a_boolean() {
    let parsed = parse_arguments(&scanned("list --open"), &[]);

    assert!(parsed.is_set("open"));
    assert_eq!(parsed.args, vec!["list"]);
}

#[test]
fn a_json_tail_is_read_from_what_was_written() {
    let parsed = parse_arguments(&scanned(r#"daily with {"width": 320}"#), &[]);

    // the tokens themselves have lost their quotes, which is what quoting is for; the raw tail has
    // not, which is what keeps an unquoted payload parseable.
    assert_eq!(
        parsed.raw_after("with").as_deref(),
        Some(r#"{"width": 320}"#)
    );
    assert_eq!(parsed.raw_after("without"), None);
}

#[test]
fn a_console_verb_is_matched_before_the_clap_surface() {
    let matched = match_meta(&scanned("run workflow daily --param a=1")).expect("a console verb");

    assert_eq!(matched.command.path, ["run", "workflow"]);
    assert_eq!(matched.arguments.args, vec!["daily"]);
    assert_eq!(matched.arguments.flag_list("param"), ["a=1".to_string()]);
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
fn completes_a_console_verbs_second_word() {
    assert_eq!(
        complete(":run ").options,
        vec!["pipeline".to_string(), "workflow".to_string()]
    );
}

#[test]
fn completes_the_values_a_flag_accepts() {
    let offered = complete(":replicas list --status ").options;
    assert_eq!(offered, vec!["live", "offline", "stale"]);

    let narrowed = complete(":replicas list --status li").options;
    assert_eq!(narrowed, vec!["live".to_string()]);
}

#[test]
fn hints_at_a_value_that_cannot_be_completed() {
    // a flag whose values are open-ended still says what it wants.
    let flag = complete(":runs list --status ");
    assert!(flag.options.is_empty());
    assert!(flag.hint.expect("a hint").contains("--status"));

    // so does a positional, whether it belongs to a console verb or a command-line one.
    assert!(
        complete(":workflows show ")
            .hint
            .expect("a hint")
            .contains("WORKFLOW")
    );
    assert!(
        complete(":run workflow ")
            .hint
            .expect("a hint")
            .contains("workflow")
    );
}

#[test]
fn a_finished_command_offers_nothing_and_hints_nothing() {
    let completion = complete(":workflows list ");

    assert!(completion.options.is_empty());
    assert_eq!(completion.hint, None);
}

#[test]
fn a_completion_replaces_only_the_word_being_typed() {
    let line = ":workflows appl";
    assert_eq!(&line[complete(line).start..], "appl");
}
