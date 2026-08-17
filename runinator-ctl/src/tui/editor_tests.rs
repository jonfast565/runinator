//! covers the prompt's editing state: typing, submitting, history, and completion.

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn control(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

fn type_line(editor: &mut PromptEditor, text: &str) {
    for character in text.chars() {
        editor.handle(key(KeyCode::Char(character)));
    }
}

#[test]
fn typing_and_submitting_a_finished_line() {
    let mut editor = PromptEditor::default();
    type_line(&mut editor, "1 + 2");

    assert_eq!(editor.buffer(), "1 + 2");
    assert_eq!(
        editor.handle(key(KeyCode::Enter)),
        Outcome::Submit("1 + 2".into())
    );
    assert!(editor.buffer().is_empty());
}

#[test]
fn enter_opens_a_line_while_a_block_is_open() {
    let mut editor = PromptEditor::default();
    type_line(&mut editor, "workflow \"x\" v1 {");

    assert_eq!(editor.handle(key(KeyCode::Enter)), Outcome::Pending);
    assert_eq!(editor.caret(), (1, 0));

    type_line(&mut editor, "}");
    assert!(matches!(
        editor.handle(key(KeyCode::Enter)),
        Outcome::Submit(_)
    ));
}

#[test]
fn shift_enter_always_opens_a_line() {
    let mut editor = PromptEditor::default();
    type_line(&mut editor, "1 + 2");
    editor.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));

    assert_eq!(editor.buffer(), "1 + 2\n");
}

#[test]
fn ctrl_c_abandons_the_line_and_ctrl_d_exits_an_empty_one() {
    let mut editor = PromptEditor::default();
    type_line(&mut editor, "half a thought");

    assert_eq!(editor.handle(control('c')), Outcome::Pending);
    assert!(editor.buffer().is_empty());
    assert_eq!(editor.handle(control('d')), Outcome::Exit);
}

#[test]
fn editing_keys_move_and_delete() {
    let mut editor = PromptEditor::default();
    type_line(&mut editor, "abcd");
    editor.handle(key(KeyCode::Left));
    editor.handle(key(KeyCode::Backspace));

    assert_eq!(editor.buffer(), "abd");

    editor.handle(control('a'));
    editor.handle(key(KeyCode::Delete));
    assert_eq!(editor.buffer(), "bd");

    editor.handle(control('e'));
    editor.handle(control('u'));
    assert!(editor.buffer().is_empty());
}

#[test]
fn arrow_up_walks_submitted_lines_and_arrow_down_returns() {
    let mut editor = PromptEditor::new(vec![":workflows list".into(), "1 + 2".into()]);

    editor.handle(key(KeyCode::Up));
    assert_eq!(editor.buffer(), "1 + 2");

    editor.handle(key(KeyCode::Up));
    assert_eq!(editor.buffer(), ":workflows list");

    editor.handle(key(KeyCode::Down));
    assert_eq!(editor.buffer(), "1 + 2");

    editor.handle(key(KeyCode::Down));
    assert!(editor.buffer().is_empty());
}

#[test]
fn a_submitted_line_joins_the_history_once() {
    let mut editor = PromptEditor::new(vec!["1 + 2".into()]);
    type_line(&mut editor, "1 + 2");
    editor.handle(key(KeyCode::Enter));

    assert_eq!(editor.history(), &["1 + 2".to_string()]);
}

#[test]
fn tab_completes_a_lone_candidate_and_lists_several() {
    let mut editor = PromptEditor::default();
    type_line(&mut editor, ":work");
    editor.handle(key(KeyCode::Tab));

    assert_eq!(editor.buffer(), ":workflows ");
    assert!(editor.menu.is_empty());

    type_line(&mut editor, "r");
    editor.handle(key(KeyCode::Tab));

    // `revision`, `revisions`, `rollback`, `run`: several, so they are offered rather than chosen.
    assert!(editor.menu.len() > 1);
    assert!(editor.menu.contains(&"rollback".to_string()));
    assert_eq!(editor.buffer(), ":workflows r");
}

#[test]
fn tab_offers_nothing_on_a_wdl_line() {
    let mut editor = PromptEditor::default();
    type_line(&mut editor, "1 + ");
    editor.handle(key(KeyCode::Tab));

    assert!(editor.menu.is_empty());
    assert_eq!(editor.buffer(), "1 + ");
}

#[test]
fn a_key_release_is_ignored() {
    let mut editor = PromptEditor::default();
    let mut press = key(KeyCode::Char('a'));
    press.kind = KeyEventKind::Release;
    editor.handle(press);

    assert!(editor.buffer().is_empty());
}
