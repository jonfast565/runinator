//! covers what the console actually draws, rendered into a test backend rather than a terminal.

use super::*;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::tui::transcript::Transcript;

/// tall enough for a status line, a few rows of output, the input, and the legend.
const ROWS: u16 = 12;

fn rendered(view: &PromptView) -> Vec<String> {
    rendered_in(view, ROWS)
}

fn rendered_in(view: &PromptView, rows: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(80, rows)).expect("test terminal builds");
    terminal
        .draw(|frame| {
            draw(frame, view);
        })
        .expect("frame draws");
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn log(lines: &[&str]) -> Transcript {
    let mut transcript = Transcript::default();
    for line in lines {
        transcript.write(&format!("{line}\n"));
    }
    transcript
}

fn view<'a>(buffer: &'a str, output: &'a Window<'a>, menu: &'a [String]) -> PromptView<'a> {
    PromptView {
        session: "scratch",
        api_base_url: "http://127.0.0.1:8080/",
        state: "ready",
        output,
        buffer,
        caret: (0, buffer.chars().count()),
        input_scroll: None,
        menu,
        hint: None,
        note: None,
    }
}

#[test]
fn the_status_line_names_the_session_and_the_service() {
    let empty = Transcript::default();
    let lines = rendered(&view("", &empty.view(4), &[]));

    assert!(lines[0].contains("runinator"));
    assert!(lines[0].contains("scratch"));
    assert!(lines[0].contains("http://127.0.0.1:8080/"));
    assert!(lines[0].contains("ready"));
}

#[test]
fn the_output_pane_shows_what_commands_printed() {
    let transcript = log(&["workflow  status", "sdlc-scan Running"]);
    let lines = rendered(&view("", &transcript.view(4), &[]));

    assert!(lines.iter().any(|line| line.contains("workflow  status")));
    assert!(lines.iter().any(|line| line.contains("sdlc-scan Running")));
}

#[test]
fn the_pane_header_counts_the_lines_it_is_showing() {
    let transcript = log(&["one", "two", "three"]);
    let lines = rendered(&view("", &transcript.view(3), &[]));

    assert!(lines.iter().any(|line| line.contains("output 1-3/3")));
}

#[test]
fn a_scrolled_pane_says_so_rather_than_looking_live() {
    let mut transcript = log(&["one", "two", "three", "four", "five", "six"]);
    transcript.scroll(-3, 2);
    let lines = rendered(&view("", &transcript.view(2), &[]));

    // a pane showing old output while a command prints is the one way this can mislead, so it is
    // labelled and the key that undoes it is named.
    assert!(lines.iter().any(|line| line.contains("scrolled")));
    assert!(lines.iter().any(|line| line.contains("Shift+End")));
    assert!(lines.iter().any(|line| line.contains("two")));
    assert!(lines.iter().any(|line| line.contains("three")));
    assert!(!lines.iter().any(|line| line.contains("six")));
}

#[test]
fn a_pane_that_dropped_lines_says_how_many() {
    let mut transcript = Transcript::with_limit(2);
    transcript.write("one\ntwo\nthree\n");
    let lines = rendered(&view("", &transcript.view(3), &[]));

    assert!(lines.iter().any(|line| line.contains("1 dropped")));
}

#[test]
fn a_rexrap_line_and_a_command_line_carry_different_sigils() {
    let empty = Transcript::default();
    let rexrap = rendered(&view("1 + 2", &empty.view(4), &[]));
    assert!(rexrap.iter().any(|line| line.contains("› 1 + 2")));

    let command = rendered(&view(":workflows list", &empty.view(4), &[]));
    assert!(
        command
            .iter()
            .any(|line| line.contains(": :workflows list"))
    );
}

#[test]
fn a_continued_line_marks_its_second_row() {
    let empty = Transcript::default();
    let lines = rendered(&view("workflow \"x\" v1 {\n  yield 1", &empty.view(4), &[]));

    assert!(
        lines
            .iter()
            .any(|line| line.contains("› workflow \"x\" v1 {"))
    );
    assert!(lines.iter().any(|line| line.contains("·   yield 1")));
}

#[test]
fn the_input_pane_can_be_scrolled_off_the_caret() {
    let empty = Transcript::default();
    let buffer = "one\ntwo\nthree\nfour\nfive";
    let window = empty.view(4);

    // following the caret shows the end of the cell.
    let following = rendered(&view(buffer, &window, &[]));
    assert!(following.iter().any(|line| line.contains("five")));
    assert!(!following.iter().any(|line| line.contains("› one")));

    // scrolled by hand it shows the top of it instead.
    let mut scrolled = view(buffer, &window, &[]);
    scrolled.input_scroll = Some(0);
    let scrolled = rendered(&scrolled);
    assert!(scrolled.iter().any(|line| line.contains("› one")));
    assert!(!scrolled.iter().any(|line| line.contains("five")));
}

#[test]
fn the_menu_shows_the_candidates() {
    let empty = Transcript::default();
    let menu = vec!["rollback".to_string(), "run".to_string()];
    let lines = rendered(&view(":workflows r", &empty.view(4), &menu));

    assert!(lines.iter().any(|line| line.contains("rollback")));
    assert!(lines.iter().any(|line| line.contains("run")));
}

#[test]
fn a_note_takes_the_menu_band_when_there_are_no_candidates() {
    let empty = Transcript::default();
    let window = empty.view(4);
    let mut noted = view("", &window, &[]);
    noted.note = Some("workflow 'nope' not found");
    let lines = rendered(&noted);

    assert!(
        lines
            .iter()
            .any(|line| line.contains("workflow 'nope' not found"))
    );
}

#[test]
fn the_menu_band_takes_no_rows_when_there_is_nothing_to_put_in_it() {
    let bands = bands(ratatui::layout::Rect::new(0, 0, 80, ROWS), "", 0, false);

    assert_eq!(bands.menu.height, 0);
    // status, input band, and legend are fixed, so everything else belongs to the output.
    assert_eq!(bands.output.height, ROWS - 1 - bands.input.height - 1);
}

#[test]
fn the_legend_names_the_keys_including_the_scroll_ones() {
    let empty = Transcript::default();
    let lines = rendered(&view("", &empty.view(4), &[]));
    let legend = lines.last().expect("a legend row");

    assert!(legend.contains("Enter run"));
    assert!(legend.contains("Tab complete"));
    assert!(legend.contains("PgUp/PgDn scroll"));
    assert!(legend.contains("Ctrl+D exit"));
}

#[test]
fn the_frame_still_draws_when_there_is_barely_room_for_it() {
    let empty = Transcript::default();
    // a four-row terminal cannot fit every band; it must clip rather than panic.
    let lines = rendered_in(&view("", &empty.view(1), &[]), 4);

    assert_eq!(lines.len(), 4);
}
