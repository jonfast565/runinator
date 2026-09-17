#[allow(unused_imports)]
use super::*;

#[derive(Debug, Default)]
pub(crate) struct AnsiParser {
    pub(super) style: Style,
    pub(super) pending_escape: Vec<u8>,
}

impl AnsiParser {
    pub(crate) fn parse_line(&mut self, input: &str) -> StyledLine {
        let mut result = StyledLine::default();
        let mut text = String::new();
        let mut combined = std::mem::take(&mut self.pending_escape);
        combined.extend_from_slice(input.as_bytes());
        let source = String::from_utf8_lossy(&combined);
        let bytes = source.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == 0x1b {
                self.flush(&mut result, &mut text);
                let Some(end) = skip_escape(bytes, index, &mut self.style) else {
                    self.pending_escape.extend_from_slice(&bytes[index..]);
                    break;
                };
                index = end;
                continue;
            }
            let Some(character) = source[index..].chars().next() else {
                break;
            };
            index += character.len_utf8();
            match character {
                '\t' => text.push(' '),
                character if !character.is_control() => text.push(character),
                _ => {}
            }
        }
        self.flush(&mut result, &mut text);
        while result.plain.ends_with(' ') {
            result.plain.pop();
            if let Some(last) = result.spans.last_mut() {
                last.text.pop();
                if last.text.is_empty() {
                    result.spans.pop();
                }
            }
        }
        result
    }

    pub(super) fn flush(&self, result: &mut StyledLine, text: &mut String) {
        if text.is_empty() {
            return;
        }
        result.plain.push_str(text);
        result.spans.push(StyledSpan {
            text: std::mem::take(text),
            style: self.style,
        });
    }
}
