use super::*;

fn texts(src: &str) -> Vec<String> {
    extract_comments(src)
        .into_iter()
        .map(|comment| comment.text)
        .collect()
}

#[test]
fn extracts_line_and_block_comments() {
    let comments = extract_comments("// one\n/* two */ x /* three */");
    assert_eq!(comments.len(), 3);
    assert_eq!(comments[0].kind, CommentKind::Line);
    assert_eq!(comments[0].text, "// one");
    assert_eq!(comments[1].kind, CommentKind::Block);
    assert_eq!(comments[1].text, "/* two */");
    assert_eq!(comments[2].text, "/* three */");
}

#[test]
fn line_comment_excludes_newline_and_carriage_return() {
    let comments = extract_comments("// note\r\nnext");
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "// note");
}

#[test]
fn own_line_flag_tracks_leading_whitespace() {
    let comments = extract_comments("x // trailing\n  // leading");
    assert_eq!(comments.len(), 2);
    assert!(!comments[0].own_line);
    assert!(comments[1].own_line);
}

#[test]
fn ignores_comment_markers_inside_strings() {
    // the `//` and `/*` here live inside a string literal and must not be treated as comments.
    assert!(texts("\"http://x\" \"/* not */\"").is_empty());
}

#[test]
fn ignores_markers_inside_interpolation_nested_string() {
    // interpolation re-enables expressions, which may contain nested strings holding `//`.
    assert!(texts("\"${ f(\"a//b\") }\"").is_empty());
}

#[test]
fn ignores_markers_inside_raw_block() {
    assert!(texts("```\n// not a comment\n/* nor this */\n```").is_empty());
}

#[test]
fn captures_comment_after_string() {
    let comments = extract_comments("\"value\" // real");
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "// real");
}

#[test]
fn spans_point_at_source() {
    let src = "ab // c";
    let comments = extract_comments(src);
    assert_eq!(comments.len(), 1);
    let span = comments[0].span;
    assert_eq!(&src[span.start..span.end], "// c");
}

#[test]
fn unterminated_block_comment_runs_to_end() {
    let comments = extract_comments("/* open");
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "/* open");
}
