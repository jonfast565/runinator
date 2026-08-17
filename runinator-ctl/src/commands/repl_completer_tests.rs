//! covers reedline's view of a completion: the span it replaces and the words it offers.

use super::*;

fn values(line: &str) -> Vec<String> {
    suggestions(line, line.len())
        .into_iter()
        .map(|suggestion| suggestion.value)
        .collect()
}

#[test]
fn offers_nothing_for_a_wdl_line() {
    assert!(values("1 + ").is_empty());
    assert!(values("action jira.create").is_empty());
}

#[test]
fn offers_verbs_for_a_bare_colon() {
    let offered = values(":");
    assert!(offered.contains(&"workflows".to_string()));
    assert!(offered.contains(&"bindings".to_string()));
}

#[test]
fn replaces_only_the_word_being_typed() {
    let line = ":workflows appl";
    let span = suggestions(line, line.len())
        .first()
        .expect("a suggestion")
        .span;
    assert_eq!(&line[span.start..span.end], "appl");
}
