//! terminal-console input completeness.

use super::*;

#[test]
fn multiline_validator_tracks_delimiters_and_quotes() {
    let validator = WdlValidator;
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

#[test]
fn console_commands_split_targets_from_json_inputs() {
    let (target, input) = target_and_json(r#"image.resize alias production with {"width":320}"#)
        .expect("command should parse");
    assert_eq!(target, "image.resize alias production");
    assert_eq!(input["width"], 320);

    let (target, input) = target_and_json("daily-report").expect("bare target should parse");
    assert_eq!(target, "daily-report");
    assert_eq!(input, runinator_models::json!({}));
}
