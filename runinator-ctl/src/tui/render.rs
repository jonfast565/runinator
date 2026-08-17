//! drawing the console prompt.
//!
//! the frame is four bands: a status line, the input, the completion menu, and a key legend. it is
//! an *inline* viewport, so command output keeps scrolling above it exactly as it always has —
//! nothing here has to intercept a `println!`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// the fixed height of the inline viewport, in terminal rows.
pub(crate) const VIEWPORT_ROWS: u16 = 9;

/// how many rows the input band may grow to before it scrolls internally.
const MAX_INPUT_ROWS: u16 = 4;

/// everything the prompt draws, gathered by the caller so drawing stays a pure function of it.
pub(crate) struct PromptView<'a> {
    pub session: &'a str,
    pub api_base_url: &'a str,
    /// what the console is doing, shown at the right of the status line.
    pub state: &'a str,
    pub buffer: &'a str,
    /// caret position in the buffer, as (line, column).
    pub caret: (usize, usize),
    pub menu: &'a [String],
    /// what belongs at the caret, when `Tab` had nothing to insert.
    pub hint: Option<&'a str>,
    /// a transient message: the last error, or what a command reported.
    pub note: Option<&'a str>,
}

pub(crate) fn draw(frame: &mut Frame, view: &PromptView) {
    let area = frame.area();
    let input_rows = input_rows(view.buffer);
    let [status, input, menu, legend] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(input_rows),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    frame.render_widget(status_bar(view), status);
    frame.render_widget(input_box(view, input_rows), input);
    frame.render_widget(menu_list(view), menu);
    frame.render_widget(legend_line(), legend);
    place_caret(frame, view, input, input_rows);
}

// the session on the left, what the console is doing on the right, the service in between.
fn status_bar<'a>(view: &'a PromptView<'a>) -> Paragraph<'a> {
    let line = Line::from(vec![
        Span::styled(" runinator ", Style::new().bold().reversed()),
        Span::raw(" "),
        Span::styled(view.session, Style::new().cyan().bold()),
        Span::styled(" · ", Style::new().dark_gray()),
        Span::styled(view.api_base_url, Style::new().dark_gray()),
        Span::styled(" · ", Style::new().dark_gray()),
        Span::styled(view.state, Style::new().magenta()),
    ]);
    Paragraph::new(line)
}

// the buffer, with a sigil that says which language the line is in and a continuation marker on
// every line after the first.
fn input_box<'a>(view: &'a PromptView<'a>, rows: u16) -> Paragraph<'a> {
    let command = view.buffer.trim_start().starts_with(':');
    let sigil = if command { ":" } else { "›" };
    let lines: Vec<Line> = view
        .buffer
        .split('\n')
        .enumerate()
        .map(|(index, text)| {
            let marker = if index == 0 { sigil } else { "·" };
            Line::from(vec![
                Span::styled(
                    format!("{marker} "),
                    if command {
                        Style::new().yellow()
                    } else {
                        Style::new().green()
                    },
                ),
                Span::raw(text),
            ])
        })
        .collect();

    // a buffer taller than the band scrolls so the caret stays visible, the way an editor does.
    let scroll = (lines.len() as u16).saturating_sub(rows);
    Paragraph::new(lines)
        .scroll((scroll, 0))
        // the style goes on the border alone: a block style would tint the buffer itself, and the
        // line being typed should read as ordinary terminal text.
        .block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(Style::new().dark_gray()),
        )
}

// the completion candidates, the hint for the value being typed, or the note — one band, because
// no two of them are interesting at the same moment. an error outranks a hint: it is about the line
// that just ran, and the hint is only about the one being written.
fn menu_list<'a>(view: &'a PromptView<'a>) -> Paragraph<'a> {
    if view.menu.is_empty() {
        return Paragraph::new(match (view.note, view.hint) {
            (Some(note), _) => Line::styled(note, Style::new().red()),
            (None, Some(hint)) => Line::styled(hint, Style::new().dark_gray()),
            (None, None) => Line::raw(""),
        });
    }

    let lines: Vec<Line> = view
        .menu
        .chunks(6)
        .map(|row| {
            Line::from(
                row.iter()
                    .map(|option| Span::styled(format!("{option:<18}"), Style::new().cyan()))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    Paragraph::new(lines)
}

fn legend_line<'a>() -> Paragraph<'a> {
    Paragraph::new(Line::styled(
        // kept under eighty columns so it is not truncated on a narrow terminal; the rest of the
        // bindings are in `:help`.
        "Enter run · Shift+Enter newline · Tab complete · ↑↓ history · Ctrl+D exit",
        Style::new().dark_gray().add_modifier(Modifier::DIM),
    ))
}

// the terminal's own caret sits where the buffer's does, so the block cursor lands on the character
// being edited rather than at the end of the line.
fn place_caret(frame: &mut Frame, view: &PromptView, input: Rect, rows: u16) {
    let (line, column) = view.caret;
    let total = view.buffer.split('\n').count() as u16;
    let scroll = total.saturating_sub(rows);
    let row = (line as u16).saturating_sub(scroll);
    // the band's first row is its top border.
    let y = input.y + 1 + row.min(rows.saturating_sub(1));
    let x = input.x + 2 + column as u16;
    frame.set_cursor_position(Position::new(x.min(input.right().saturating_sub(1)), y));
}

fn input_rows(buffer: &str) -> u16 {
    // the border costs a row, so the band is one taller than the text it shows.
    (buffer.split('\n').count() as u16).clamp(1, MAX_INPUT_ROWS) + 1
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
