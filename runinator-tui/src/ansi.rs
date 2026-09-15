//! Incremental ANSI SGR decoding for log text rendered by ratatui.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StyledLine {
    pub(crate) plain: String,
    spans: Vec<StyledSpan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StyledSpan {
    text: String,
    style: Style,
}

impl StyledLine {
    pub(crate) fn to_ratatui_line(&self) -> Line<'static> {
        Line::from(
            self.spans
                .iter()
                .map(|span| Span::styled(span.text.clone(), span.style))
                .collect::<Vec<_>>(),
        )
    }
}

#[derive(Debug, Default)]
pub(crate) struct AnsiParser {
    style: Style,
    pending_escape: Vec<u8>,
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

    fn flush(&self, result: &mut StyledLine, text: &mut String) {
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

fn skip_escape(bytes: &[u8], start: usize, style: &mut Style) -> Option<usize> {
    let kind = bytes.get(start + 1).copied()?;
    if kind == b'[' {
        let mut end = start + 2;
        while end < bytes.len() && !(0x40..=0x7e).contains(&bytes[end]) {
            end += 1;
        }
        if end >= bytes.len() {
            return None;
        }
        if bytes[end] == b'm' {
            apply_sgr(&bytes[start + 2..end], style);
        }
        return Some(end + 1);
    }
    if kind == b']' {
        let mut end = start + 2;
        while end < bytes.len() {
            if bytes[end] == 0x07 {
                return Some(end + 1);
            }
            if bytes[end] == 0x1b && bytes.get(end + 1) == Some(&b'\\') {
                return Some(end + 2);
            }
            end += 1;
        }
        return None;
    }
    Some((start + 2).min(bytes.len()))
}

fn apply_sgr(parameters: &[u8], style: &mut Style) {
    let mut values = if parameters.is_empty() {
        vec![0]
    } else {
        String::from_utf8_lossy(parameters)
            .split(';')
            .map(|value| value.parse::<u16>().unwrap_or_default())
            .collect::<Vec<_>>()
    };
    let mut index = 0;
    while index < values.len() {
        match values[index] {
            0 => *style = Style::default(),
            1 => style.add_modifier |= Modifier::BOLD,
            2 => style.add_modifier |= Modifier::DIM,
            3 => style.add_modifier |= Modifier::ITALIC,
            4 => style.add_modifier |= Modifier::UNDERLINED,
            22 => style.sub_modifier |= Modifier::BOLD | Modifier::DIM,
            23 => style.sub_modifier |= Modifier::ITALIC,
            24 => style.sub_modifier |= Modifier::UNDERLINED,
            30..=37 => style.fg = Some(color(values[index] - 30, false)),
            39 => style.fg = None,
            40..=47 => style.bg = Some(color(values[index] - 40, false)),
            49 => style.bg = None,
            90..=97 => style.fg = Some(color(values[index] - 90, true)),
            100..=107 => style.bg = Some(color(values[index] - 100, true)),
            38 | 48 => {
                let foreground = values[index] == 38;
                let parsed = extended_color(&values, index + 1);
                if let Some((color, consumed)) = parsed {
                    if foreground {
                        style.fg = Some(color);
                    } else {
                        style.bg = Some(color);
                    }
                    index += consumed;
                }
            }
            _ => {}
        }
        index += 1;
    }
    values.clear();
}

fn extended_color(values: &[u16], start: usize) -> Option<(Color, usize)> {
    match values.get(start).copied()? {
        5 => Some((
            Color::Indexed(u8::try_from(*values.get(start + 1)?).ok()?),
            2,
        )),
        2 => Some((
            Color::Rgb(
                u8::try_from(*values.get(start + 1)?).ok()?,
                u8::try_from(*values.get(start + 2)?).ok()?,
                u8::try_from(*values.get(start + 3)?).ok()?,
            ),
            4,
        )),
        _ => None,
    }
}

fn color(index: u16, bright: bool) -> Color {
    match (index, bright) {
        (0, false) => Color::Black,
        (1, false) => Color::Red,
        (2, false) => Color::Green,
        (3, false) => Color::Yellow,
        (4, false) => Color::Blue,
        (5, false) => Color::Magenta,
        (6, false) => Color::Cyan,
        (7, false) => Color::Gray,
        (0, true) => Color::DarkGray,
        (1, true) => Color::LightRed,
        (2, true) => Color::LightGreen,
        (3, true) => Color::LightYellow,
        (4, true) => Color::LightBlue,
        (5, true) => Color::LightMagenta,
        (6, true) => Color::LightCyan,
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_sgr_colors_and_plain_search_text() {
        let mut parser = AnsiParser::default();
        let line = parser.parse_line("\u{1b}[1;31mfailed\u{1b}[0m\t now");
        assert_eq!(line.plain, "failed  now");
        assert_eq!(line.spans[0].style.fg, Some(Color::Red));
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn strips_cursor_and_osc_controls() {
        let mut parser = AnsiParser::default();
        let line = parser.parse_line("a\u{1b}[2Kb\u{1b}]0;secret\u{7}c");
        assert_eq!(line.plain, "abc");
    }

    #[test]
    fn style_carries_across_physical_lines() {
        let mut parser = AnsiParser::default();
        parser.parse_line("\u{1b}[32mfirst");
        let second = parser.parse_line("second\u{1b}[0m");
        assert_eq!(second.spans[0].style.fg, Some(Color::Green));
    }

    #[test]
    fn fragmented_escape_is_completed_by_the_next_chunk() {
        let mut parser = AnsiParser::default();
        let first = parser.parse_line("before\u{1b}[3");
        assert_eq!(first.plain, "before");
        let second = parser.parse_line("1mred\u{1b}[0m");
        assert_eq!(second.plain, "red");
        assert_eq!(second.spans[0].style.fg, Some(Color::Red));
    }
}
