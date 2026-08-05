//! comments survive a format round trip and stay attached to the declaration or branch they
//! were written against.

use super::*;

fn find_at(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("expected to find {needle:?} in:\n{haystack}"))
}
#[test]
fn comments_survive_format_round_trip() {
    let src = "// top of file\n\
               workflow \"demo\" v1 {\n\
               \x20 // schedule\n\
               \x20 trigger cron \"0 * * * *\"\n\
               \n\
               \x20 // greet the user\n\
               \x20 node greet <- console.run(command: \"hi\") // inline note\n\
               \x20 node bye <- console.run(command: \"bye\")\n\
               }\n\
               // end of file\n";
    let out = format_str(src).expect("format");
    // every comment is preserved.
    for comment in [
        "// top of file",
        "// schedule",
        "// greet the user",
        "// inline note",
        "// end of file",
    ] {
        assert!(out.contains(comment), "missing {comment:?} in:\n{out}");
    }
    // the trigger keeps its own leading comment; the node keeps its leading and its inline trailing.
    assert!(find_at(&out, "// schedule") < find_at(&out, "trigger cron"));
    assert!(find_at(&out, "// greet the user") < find_at(&out, "node greet"));
    assert!(find_at(&out, "node greet") < find_at(&out, "// inline note"));
    assert!(find_at(&out, "// end of file") > find_at(&out, "node bye"));
    // formatting is idempotent once comments are attached.
    assert_eq!(format_str(&out).expect("reformat"), out);
}
#[test]
fn branch_comments_stay_in_their_branch() {
    let src = "workflow \"br\" v1 {\n\
               \x20 if cond(\"true\") {\n\
               \x20   node a <- console.run(command: \"a\")\n\
               \x20   // note in if\n\
               \x20 } else {\n\
               \x20   // note in else\n\
               \x20   node b <- console.run(command: \"b\")\n\
               \x20 }\n\
               }\n";
    let out = format_str(src).expect("format");
    // the if-branch comment stays above the else keyword; the else comment stays below it.
    let else_at = find_at(&out, "} else {");
    assert!(
        find_at(&out, "// note in if") < else_at,
        "if-note escaped:\n{out}"
    );
    assert!(
        find_at(&out, "// note in else") > else_at,
        "else-note escaped:\n{out}"
    );
    assert!(find_at(&out, "// note in else") < find_at(&out, "node b"));
    assert_eq!(format_str(&out).expect("reformat"), out);
}
#[test]
fn header_comments_track_their_declaration() {
    let src = "workflow \"multi\" v1 {\n\
               \x20 import slack // we need slack\n\
               \x20 // first\n\
               \x20 import github\n\
               \n\
               \x20 node a <- console.run(command: \"a\")\n\
               }\n";
    let out = format_str(src).expect("format");
    assert!(find_at(&out, "import slack") < find_at(&out, "// we need slack"));
    assert!(find_at(&out, "// we need slack") < find_at(&out, "// first"));
    assert!(find_at(&out, "// first") < find_at(&out, "import github"));
    assert_eq!(format_str(&out).expect("reformat"), out);
}
#[test]
fn comment_markers_inside_strings_are_not_treated_as_comments() {
    // the `//` lives inside a string literal, so the formatter must not hoist it out as a comment.
    let src = "workflow \"s\" v1 {\n\
               \x20 node a <- console.run(command: \"see http://example.com\")\n\
               }\n";
    let out = format_str(src).expect("format");
    assert!(out.contains("http://example.com"));
    // no stray comment line was invented.
    assert!(!out.contains("// example"));
    assert_eq!(format_str(&out).expect("reformat"), out);
}
#[test]
fn commentless_source_still_round_trips() {
    let src = "workflow \"plain\" v1 {\n\
               \x20 node a <- console.run(command: \"a\")\n\
               }\n";
    let out = format_str(src).expect("format");
    assert!(!out.contains("//"));
    assert_eq!(format_str(&out).expect("reformat"), out);
}
#[test]
fn params_field_comments_are_preserved() {
    let src = "workflow \"p\" v1 {\n\
               \x20 params {\n\
               \x20   // the user's name\n\
               \x20   name: string\n\
               \x20   count: int = 3 // how many times\n\
               \x20   // nested config\n\
               \x20   config: {\n\
               \x20     // enable feature\n\
               \x20     enabled: bool\n\
               \x20   }\n\
               \x20   // trailing param note\n\
               \x20 }\n\
               \n\
               \x20 node a <- console.run(command: params.name)\n\
               }\n";
    let out = format_str(src).expect("format");
    // field-level leading comment stays with its field.
    assert!(find_at(&out, "// the user's name") < find_at(&out, "name: string"));
    // inline trailing comment stays on the field line, after it.
    assert!(find_at(&out, "count: int = 3") < find_at(&out, "// how many times"));
    // nested struct field comment stays inside the nested block, above its field.
    assert!(find_at(&out, "// enable feature") < find_at(&out, "enabled: bool"));
    // a comment after the last field renders before the params block's closing brace, not below it.
    let dangling = find_at(&out, "// trailing param note");
    assert!(dangling > find_at(&out, "enabled: bool"));
    assert!(dangling < find_at(&out, "node a"));
    assert_eq!(format_str(&out).expect("reformat"), out);
}
#[test]
fn type_decl_field_comments_are_preserved() {
    let src = "workflow \"t\" v1 {\n\
               \x20 params { r: Rec }\n\
               \x20 // shape of a record\n\
               \x20 type Rec {\n\
               \x20   // primary key\n\
               \x20   id: string\n\
               \x20   label: string // human label\n\
               \x20 }\n\
               \x20 node a <- console.run(command: \"x\")\n\
               }\n";
    let out = format_str(src).expect("format");
    assert!(find_at(&out, "// shape of a record") < find_at(&out, "type Rec"));
    assert!(find_at(&out, "// primary key") < find_at(&out, "id: string"));
    assert!(find_at(&out, "label: string") < find_at(&out, "// human label"));
    assert_eq!(format_str(&out).expect("reformat"), out);
}
