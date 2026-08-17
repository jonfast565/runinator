//! drawing the console.
//!
//! the frame is five bands: a status line, the scrollable output pane, the input, the completion
//! menu, and a key legend. the layout is a pure function of the area, the buffer, and how many rows
//! the menu wants, so `bands` is what both drawing and mouse hit-testing read — a second copy of
//! the arithmetic would put the wheel a row off from what it looks like it is over.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::transcript::Window;

/// how many rows the input band may grow to before it scrolls internally.
const MAX_INPUT_ROWS: u16 = 4;

/// how many rows the completion menu may take.
const MAX_MENU_ROWS: u16 = 3;

/// how many candidates the menu puts on one row.
const MENU_COLUMNS: usize = 6;

/// where each band of the frame sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bands {
    pub status: Rect,
    /// the output band, top border included.
    pub output: Rect,
    /// the input band, top border included.
    pub input: Rect,
    pub menu: Rect,
    pub legend: Rect,
}

impl Bands {
    /// the rows of the output band that hold text, which is the pane height every scroll is in
    /// terms of.
    pub(crate) fn output_lines(&self) -> Rect {
        inner(self.output)
    }

    /// the pane the pointer at `position` is over, when it is over one that scrolls.
    pub(crate) fn pane_at(&self, position: Position) -> Option<Pane> {
        if self.output.contains(position) {
            return Some(Pane::Output);
        }
        if self.input.contains(position) || self.menu.contains(position) {
            return Some(Pane::Input);
        }
        None
    }
}

/// the two halves of the console that scroll independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pane {
    Output,
    Input,
}

/// everything the console draws, gathered by the caller so drawing stays a pure function of it.
pub(crate) struct PromptView<'a> {
    pub session: &'a str,
    pub api_base_url: &'a str,
    /// what the console is doing, shown at the right of the status line.
    pub state: &'a str,
    /// the retained output and where the pane is looking.
    pub output: &'a Window<'a>,
    pub buffer: &'a str,
    /// caret position in the buffer, as (line, column).
    pub caret: (usize, usize),
    /// the first visible buffer line, when the input pane has been scrolled by hand. `None` follows
    /// the caret, which is what typing does.
    pub input_scroll: Option<u16>,
    pub menu: &'a [String],
    /// what belongs at the caret, when `Tab` had nothing to insert.
    pub hint: Option<&'a str>,
    /// a transient message: the last error, or what a command reported.
    pub note: Option<&'a str>,
}

/// where the bands fall for an area, an input buffer, and a menu of `candidates` entries.
pub(crate) fn bands(area: Rect, buffer: &str, candidates: usize, footer: bool) -> Bands {
    let [status, output, input, menu, legend] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(input_rows(buffer)),
        Constraint::Length(menu_rows(candidates, footer)),
        Constraint::Length(1),
    ])
    .areas(area);
    Bands {
        status,
        output,
        input,
        menu,
        legend,
    }
}

pub(crate) fn draw(frame: &mut Frame, view: &PromptView) -> Bands {
    let bands = bands(
        frame.area(),
        view.buffer,
        view.menu.len(),
        view.note.is_some() || view.hint.is_some(),
    );

    frame.render_widget(status_bar(view), bands.status);
    frame.render_widget(output_pane(view), bands.output);
    frame.render_widget(input_box(view, bands.input.height), bands.input);
    frame.render_widget(menu_list(view), bands.menu);
    frame.render_widget(legend_line(), bands.legend);
    place_caret(frame, view, bands.input);
    bands
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

// the retained command output. it does not wrap: the console prints tables, and a wrapped table is
// unreadable in a way a truncated one is not — the pane scrolls sideways instead.
fn output_pane<'a>(view: &'a PromptView<'a>) -> Paragraph<'a> {
    let window = view.output;
    let lines: Vec<Line> = window.lines.iter().map(|line| Line::raw(*line)).collect();
    Paragraph::new(lines)
        .scroll((0, window.column as u16))
        .block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(Style::new().dark_gray())
                .title(position_title(window)),
        )
}

// what the pane is showing, and — when it has been scrolled back — a marker saying so, since a
// stale pane that looks live is the one way this can mislead.
fn position_title<'a>(window: &Window<'a>) -> Line<'a> {
    if window.total == 0 {
        return Line::styled(" output ", Style::new().dark_gray());
    }
    let last = window.first + window.lines.len().saturating_sub(1);
    let mut spans = vec![Span::styled(
        format!(" output {}-{}/{} ", window.first, last, window.total),
        Style::new().dark_gray(),
    )];
    if window.dropped > 0 {
        spans.push(Span::styled(
            format!("({} dropped) ", window.dropped),
            Style::new().dark_gray(),
        ));
    }
    if !window.following {
        spans.push(Span::styled(
            "↑ scrolled · Shift+End follows ",
            Style::new().yellow(),
        ));
    }
    Line::from(spans)
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

    Paragraph::new(lines)
        .scroll((input_scroll(view, rows), 0))
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
        .chunks(MENU_COLUMNS)
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
        "Enter run · Tab complete · ↑↓ history · PgUp/PgDn scroll · Ctrl+D exit",
        Style::new().dark_gray().add_modifier(Modifier::DIM),
    ))
}

// the terminal's own caret sits where the buffer's does, so the block cursor lands on the character
// being edited rather than at the end of the line.
fn place_caret(frame: &mut Frame, view: &PromptView, input: Rect) {
    let (line, column) = view.caret;
    let scroll = input_scroll(view, input.height);
    let rows = inner(input).height;
    let row = (line as u16).saturating_sub(scroll);
    // the band's first row is its top border.
    let y = input.y + 1 + row.min(rows.saturating_sub(1));
    let x = input.x + 2 + column as u16;
    frame.set_cursor_position(Position::new(x.min(input.right().saturating_sub(1)), y));
}

// a buffer taller than the band scrolls so the caret stays visible, the way an editor does — unless
// the pane was scrolled by hand, in which case it stays where it was put until the next keystroke.
fn input_scroll(view: &PromptView, rows: u16) -> u16 {
    let lines = view.buffer.split('\n').count() as u16;
    let text = rows.saturating_sub(1).max(1);
    let ceiling = lines.saturating_sub(text);
    view.input_scroll.unwrap_or(ceiling).min(ceiling)
}

/// how tall the input band is for a buffer, top border included.
pub(crate) fn input_rows(buffer: &str) -> u16 {
    // the border costs a row, so the band is one taller than the text it shows.
    (buffer.split('\n').count() as u16).clamp(1, MAX_INPUT_ROWS) + 1
}

/// how tall the menu band is for a given number of candidates.
pub(crate) fn menu_rows(candidates: usize, footer: bool) -> u16 {
    if candidates == 0 {
        // the note and the hint share the band, and both are one line.
        return u16::from(footer);
    }
    (candidates.div_ceil(MENU_COLUMNS) as u16).min(MAX_MENU_ROWS)
}

// a band's rows below its top border.
fn inner(band: Rect) -> Rect {
    Rect {
        y: band.y.saturating_add(1),
        height: band.height.saturating_sub(1),
        ..band
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
