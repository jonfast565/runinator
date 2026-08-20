//! terminal-console input completeness.

use super::*;

#[test]
fn multiline_validator_tracks_delimiters_and_quotes() {
    let validator = RexRapValidator;
    assert!(matches!(
        validator.validate("workflow \"x\" {"),
        ValidationResult::Incomplete
    ));
    assert!(matches!(
        validator.validate("workflow \"x\" {\n yield { value: 1 }\n}"),
        ValidationResult::Complete
    ));
    assert!(matches!(
        validator.validate("\"unfinished"),
        ValidationResult::Incomplete
    ));
}

fn arguments(line: &str) -> repl::Arguments {
    let tokens = repl::scan(line).expect("line scans");
    repl::match_meta(&tokens)
        .expect("a console verb matches")
        .arguments
}

#[test]
fn a_run_takes_parameters_from_either_spelling() {
    let with = arguments(r#"run workflow daily with {"width":320}"#);
    assert_eq!(run_parameters(&with).expect("json parses")["width"], 320);

    let flags = arguments("run workflow daily --param width=320");
    assert_eq!(run_parameters(&flags).expect("flags parse")["width"], 320);

    let bare = arguments("run workflow daily");
    assert_eq!(
        run_parameters(&bare).expect("an empty payload parses"),
        runinator_models::json!({})
    );
}

#[test]
fn invoke_reads_a_selector_from_flags_or_bare_words() {
    let flags = arguments("invoke image.resize --alias production");
    assert_eq!(
        function_selector(&flags).expect("selector parses"),
        (Some("production".into()), None)
    );

    let bare = arguments("invoke image.resize version 3");
    assert_eq!(
        function_selector(&bare).expect("selector parses"),
        (None, Some(3))
    );
}
