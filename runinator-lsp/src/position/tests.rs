use super::*;
use runinator_wdl::Span;
use tower_lsp::lsp_types::Position;

fn pos(line: u32, character: u32) -> Position {
    Position { line, character }
}

#[test]
fn ascii_round_trips() {
    let text = "workflow \"A\" v1 {\n  console.run(command: \"hi\")\n}\n";
    for offset in 0..=text.len() {
        if !text.is_char_boundary(offset) {
            continue;
        }
        let position = byte_to_position(text, offset);
        assert_eq!(position_to_byte(text, position), offset, "offset {offset}");
    }
}

#[test]
fn second_line_position_maps_to_byte() {
    let text = "abc\ndef";
    // line 1, char 1 -> the 'e'
    let byte = position_to_byte(text, pos(1, 1));
    assert_eq!(&text[byte..byte + 1], "e");
    assert_eq!(byte_to_position(text, byte), pos(1, 1));
}

#[test]
fn multibyte_emoji_counts_utf16_units() {
    // "a😀b": 'a'=1 byte/1 utf16, '😀'=4 bytes/2 utf16, 'b'=1 byte/1 utf16.
    let text = "a😀b";
    let byte_b = text.find('b').unwrap();
    // 'b' sits at utf-16 column 3 (1 for 'a' + 2 for the emoji).
    assert_eq!(byte_to_position(text, byte_b), pos(0, 3));
    assert_eq!(position_to_byte(text, pos(0, 3)), byte_b);
}

#[test]
fn crlf_lines_advance_on_newline() {
    let text = "ab\r\ncd";
    let byte = position_to_byte(text, pos(1, 0));
    // line 1 starts right after the '\n'.
    assert_eq!(&text[byte..byte + 1], "c");
}

#[test]
fn span_maps_to_range() {
    let text = "abc\ndef";
    let span = Span::new(4, 7);
    let range = span_to_range(text, span);
    assert_eq!(range.start, pos(1, 0));
    assert_eq!(range.end, pos(1, 3));
}

#[test]
fn character_past_line_end_clamps_to_newline() {
    let text = "ab\ncd";
    // asking for column 99 on line 0 should clamp to end of "ab".
    assert_eq!(position_to_byte(text, pos(0, 99)), 2);
}
