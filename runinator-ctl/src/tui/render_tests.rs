//! covers what the prompt actually draws, rendered into a test backend rather than a terminal.

use super::*;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn rendered(view: &PromptView) -> Vec<String> {
    let mut terminal =
        Terminal::new(TestBackend::new(80, VIEWPORT_ROWS)).expect("test terminal builds");
    terminal
        .draw(|frame| draw(frame, view))
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

fn view<'a>(buffer: &'a str, menu: &'a [String], note: Option<&'a str>) -> PromptView<'a> {
    PromptView {
        session: "scratch",
        api_base_url: "http://127.0.0.1:8080/",
        state: "ready",
        buffer,
        caret: (0, buffer.chars().count()),
        menu,
        note,
    }
}

#[test]
fn the_status_line_names_the_session_and_the_service() {
    let lines = rendered(&view("", &[], None));

    assert!(lines[0].contains("runinator"));
    assert!(lines[0].contains("scratch"));
    assert!(lines[0].contains("http://127.0.0.1:8080/"));
    assert!(lines[0].contains("ready"));
}

#[test]
fn a_wdl_line_and_a_command_line_carry_different_sigils() {
    let wdl = rendered(&view("1 + 2", &[], None));
    assert!(wdl.iter().any(|line| line.contains("› 1 + 2")));

    let command = rendered(&view(":workflows list", &[], None));
    assert!(
        command
            .iter()
            .any(|line| line.contains(": :workflows list"))
    );
}

#[test]
fn a_continued_line_marks_its_second_row() {
    let lines = rendered(&view("workflow \"x\" v1 {\n  yield 1", &[], None));

    assert!(
        lines
            .iter()
            .any(|line| line.contains("› workflow \"x\" v1 {"))
    );
    assert!(lines.iter().any(|line| line.contains("·   yield 1")));
}

#[test]
fn the_menu_shows_the_candidates() {
    let menu = vec!["rollback".to_string(), "run".to_string()];
    let lines = rendered(&view(":workflows r", &menu, None));

    assert!(lines.iter().any(|line| line.contains("rollback")));
    assert!(lines.iter().any(|line| line.contains("run")));
}

#[test]
fn a_note_takes_the_menu_band_when_there_are_no_candidates() {
    let lines = rendered(&view("", &[], Some("workflow 'nope' not found")));

    assert!(
        lines
            .iter()
            .any(|line| line.contains("workflow 'nope' not found"))
    );
}

#[test]
fn the_legend_names_the_keys() {
    let lines = rendered(&view("", &[], None));
    let legend = lines.last().expect("a legend row");

    assert!(legend.contains("Enter run"));
    assert!(legend.contains("Tab complete"));
    assert!(legend.contains("Ctrl+D exit"));
}
