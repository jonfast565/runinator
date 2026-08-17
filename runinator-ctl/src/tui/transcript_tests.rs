//! covers what the output pane holds and what it shows at a scroll position.

use super::*;

fn filled(lines: usize) -> Transcript {
    let mut transcript = Transcript::with_limit(LINE_LIMIT);
    for index in 1..=lines {
        transcript.write(&format!("line {index}\n"));
    }
    transcript
}

#[test]
fn output_becomes_lines_and_a_half_written_one_counts() {
    let mut transcript = Transcript::default();
    transcript.write("first\nsecond\nthi");

    assert_eq!(transcript.rows(), 3);
    assert_eq!(transcript.view(10).lines, vec!["first", "second", "thi"]);

    // the rest of the line arrives later and joins what was already there.
    transcript.write("rd\n");
    assert_eq!(transcript.view(10).lines, vec!["first", "second", "third"]);
}

#[test]
fn a_carriage_return_rewrites_the_line_it_is_on() {
    let mut transcript = Transcript::default();
    transcript.write("waiting 1\rwaiting 2\rdone\n");

    assert_eq!(transcript.view(10).lines, vec!["done"]);
}

#[test]
fn styling_sequences_are_dropped_rather_than_stored() {
    let mut transcript = Transcript::default();
    transcript.write("\x1b[1;32mgreen\x1b[0m and plain\n");

    assert_eq!(transcript.view(10).lines, vec!["green and plain"]);
}

#[test]
fn a_sequence_split_across_two_chunks_is_still_one_sequence() {
    let mut transcript = Transcript::default();
    transcript.write("a\x1b[3");
    transcript.write("1mb\n");

    assert_eq!(transcript.view(10).lines, vec!["ab"]);
}

#[test]
fn an_erase_display_clears_the_log_so_clear_still_works() {
    let mut transcript = filled(4);
    // what `:clear` prints.
    transcript.write("\x1b[2J\x1b[H");

    assert_eq!(transcript.rows(), 0);
    assert!(transcript.view(10).lines.is_empty());
}

#[test]
fn the_view_shows_the_newest_lines_while_it_is_following() {
    let transcript = filled(20);
    let window = transcript.view(3);

    assert_eq!(window.lines, vec!["line 18", "line 19", "line 20"]);
    assert_eq!(window.first, 18);
    assert_eq!(window.total, 20);
    assert!(window.following);
}

#[test]
fn scrolling_back_moves_the_view_and_says_it_is_no_longer_following() {
    let mut transcript = filled(20);
    transcript.scroll(-2, 3);
    let window = transcript.view(3);

    assert_eq!(window.lines, vec!["line 16", "line 17", "line 18"]);
    assert_eq!(window.first, 16);
    assert!(!window.following);
}

#[test]
fn output_arriving_while_scrolled_back_leaves_the_view_where_it_was() {
    let mut transcript = filled(20);
    transcript.scroll(-5, 3);
    let before: Vec<String> = transcript
        .view(3)
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect();

    transcript.write("line 21\nline 22\n");

    // this is the whole point of the offset being measured from the tail: reading old output does
    // not become a fight with a command that is still printing.
    assert_eq!(transcript.view(3).lines, before);
    assert!(!transcript.following());
}

#[test]
fn following_resumes_at_the_tail_however_far_back_the_view_was() {
    let mut transcript = filled(20);
    transcript.scroll(-10, 3);
    transcript.follow();

    assert!(transcript.following());
    assert_eq!(
        transcript.view(3).lines,
        vec!["line 18", "line 19", "line 20"]
    );
}

#[test]
fn a_page_overlaps_by_one_row_so_nothing_falls_between_two_pages() {
    let mut transcript = filled(20);
    transcript.scroll_pages(-1, 5);

    // five rows a page, one of them repeated from the page below.
    assert_eq!(transcript.view(5).first, 12);
}

#[test]
fn the_top_and_the_tail_are_both_reachable_in_one_step() {
    let mut transcript = filled(20);

    transcript.rewind(4);
    assert_eq!(transcript.view(4).first, 1);

    transcript.follow();
    assert_eq!(transcript.view(4).first, 17);
}

#[test]
fn scrolling_stops_at_both_ends() {
    let mut transcript = filled(6);

    transcript.scroll(-100, 4);
    assert_eq!(transcript.view(4).first, 1);

    transcript.scroll(100, 4);
    assert!(transcript.following());
}

#[test]
fn a_pane_that_grew_taller_than_the_offset_is_clamped_back_to_the_top() {
    let mut transcript = filled(10);
    transcript.scroll(-6, 2);
    assert_eq!(transcript.view(2).first, 3);

    // the terminal was made taller, so there is no longer that much to scroll past.
    transcript.clamp(10);
    assert!(transcript.following());
}

#[test]
fn the_oldest_lines_are_dropped_when_the_log_is_full_and_the_pane_says_so() {
    let mut transcript = Transcript::with_limit(3);
    transcript.write("one\ntwo\nthree\nfour\n");
    let window = transcript.view(10);

    assert_eq!(window.lines, vec!["two", "three", "four"]);
    assert_eq!(window.total, 3);
    assert_eq!(window.dropped, 1);
}

#[test]
fn sideways_scrolling_stops_at_the_left_edge() {
    let mut transcript = filled(2);

    transcript.scroll_columns(-COLUMN_STEP);
    assert_eq!(transcript.view(2).column, 0);

    transcript.scroll_columns(COLUMN_STEP);
    assert_eq!(transcript.view(2).column, COLUMN_STEP as usize);
}

#[test]
fn the_replay_is_every_retained_line_in_order() {
    let mut transcript = filled(3);
    transcript.write("half");

    assert_eq!(
        transcript.replay(),
        vec!["line 1", "line 2", "line 3", "half"]
    );
}
