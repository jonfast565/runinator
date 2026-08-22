//! the console prompt's editing state.
//!
//! pure: it holds characters, a cursor, a history, and a completion menu, and it answers key
//! events. nothing here touches a terminal, which is what lets the whole interaction be tested
//! without one.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::commands::repl;

/// what the repl loop should do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// keep editing.
    Pending,
    /// the line is finished; run it.
    Submit(String),
    /// leave the console.
    Exit,
    /// clear the scrollback.
    ClearScreen,
}

/// how many past lines the prompt remembers within one session.
const HISTORY_LIMIT: usize = 500;

#[derive(Debug, Default)]
pub(crate) struct PromptEditor {
    // characters rather than bytes: every cursor move would otherwise have to reason about utf-8
    // boundaries, and a paste with an accent in it would panic on a slice.
    characters: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    // where arrow-up currently is in the history; `None` means "editing a fresh line".
    recall: Option<usize>,
    draft: String,
    /// the completion candidates shown under the input, if any.
    pub(crate) menu: Vec<String>,
    /// what belongs at the caret when there is nothing to complete: the value's name and what it
    /// means. shown instead of the menu, since the two are never interesting at once.
    pub(crate) hint: Option<String>,
}

impl PromptEditor {
    pub(crate) fn new(history: Vec<String>) -> Self {
        Self {
            history,
            ..Self::default()
        }
    }

    pub(crate) fn buffer(&self) -> String {
        self.characters.iter().collect()
    }

    /// the caret as a (line, column) pair, for placing the terminal cursor.
    pub(crate) fn caret(&self) -> (usize, usize) {
        let mut line = 0;
        let mut column = 0;
        for character in &self.characters[..self.cursor] {
            if *character == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }
        }
        (line, column)
    }

    pub(crate) fn history(&self) -> &[String] {
        &self.history
    }

    /// handle one key event.
    pub(crate) fn handle(&mut self, key: KeyEvent) -> Outcome {
        // windows reports both press and release; acting on both would double every character.
        if key.kind == KeyEventKind::Release {
            return Outcome::Pending;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control {
            return self.control_key(key.code);
        }
        match key.code {
            KeyCode::Char(character) => {
                self.insert(character);
                Outcome::Pending
            }
            KeyCode::Enter => self.enter(key.modifiers),
            KeyCode::Tab => {
                self.complete();
                Outcome::Pending
            }
            KeyCode::Backspace => {
                self.backspace();
                Outcome::Pending
            }
            KeyCode::Delete => {
                self.delete();
                Outcome::Pending
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                Outcome::Pending
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.characters.len());
                Outcome::Pending
            }
            KeyCode::Home => {
                self.cursor = 0;
                Outcome::Pending
            }
            KeyCode::End => {
                self.cursor = self.characters.len();
                Outcome::Pending
            }
            KeyCode::Up => {
                self.recall_history(true);
                Outcome::Pending
            }
            KeyCode::Down => {
                self.recall_history(false);
                Outcome::Pending
            }
            KeyCode::Esc => {
                self.dismiss();
                Outcome::Pending
            }
            _ => Outcome::Pending,
        }
    }

    // the emacs-ish bindings a shell user expects, plus the three that mean something here.
    fn control_key(&mut self, code: KeyCode) -> Outcome {
        match code {
            // ctrl+c abandons the line rather than the console; ctrl+d on an empty line exits, which
            // is the pair every repl uses.
            KeyCode::Char('c') => {
                self.reset();
                Outcome::Pending
            }
            KeyCode::Char('d') => {
                if self.characters.is_empty() {
                    return Outcome::Exit;
                }
                self.delete();
                Outcome::Pending
            }
            KeyCode::Char('l') => Outcome::ClearScreen,
            KeyCode::Char('a') => {
                self.cursor = 0;
                Outcome::Pending
            }
            KeyCode::Char('e') => {
                self.cursor = self.characters.len();
                Outcome::Pending
            }
            KeyCode::Char('u') => {
                self.characters.drain(..self.cursor);
                self.cursor = 0;
                Outcome::Pending
            }
            KeyCode::Char('k') => {
                self.characters.truncate(self.cursor);
                Outcome::Pending
            }
            KeyCode::Char('w') => {
                self.delete_word();
                Outcome::Pending
            }
            // a newline that never submits, for the times the balance check disagrees with the
            // author.
            KeyCode::Enter | KeyCode::Char('j') => {
                self.insert('\n');
                Outcome::Pending
            }
            _ => Outcome::Pending,
        }
    }

    fn enter(&mut self, modifiers: KeyModifiers) -> Outcome {
        let buffer = self.buffer();
        if modifiers.contains(KeyModifiers::SHIFT) || modifiers.contains(KeyModifiers::ALT) {
            self.insert('\n');
            return Outcome::Pending;
        }
        if !repl::is_submittable(&buffer) {
            self.insert('\n');
            return Outcome::Pending;
        }
        self.remember(buffer.trim().to_string());
        self.reset();
        Outcome::Submit(buffer)
    }

    fn insert(&mut self, character: char) {
        self.characters.insert(self.cursor, character);
        self.cursor += 1;
        self.refresh_hint();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        self.characters.remove(self.cursor);
        self.refresh_hint();
    }

    fn delete(&mut self) {
        if self.cursor < self.characters.len() {
            self.characters.remove(self.cursor);
            self.refresh_hint();
        }
    }

    fn delete_word(&mut self) {
        let mut at = self.cursor;
        while at > 0 && self.characters[at - 1].is_whitespace() {
            at -= 1;
        }
        while at > 0 && !self.characters[at - 1].is_whitespace() {
            at -= 1;
        }
        self.characters.drain(at..self.cursor);
        self.cursor = at;
        self.refresh_hint();
    }

    fn reset(&mut self) {
        self.characters.clear();
        self.cursor = 0;
        self.recall = None;
        self.dismiss();
    }

    // the menu and the hint answer the same question, so they appear and disappear together.
    fn dismiss(&mut self) {
        self.menu.clear();
        self.hint = None;
    }

    // Hints do not need a key press: they are the prompt's quiet answer to "what goes here?".
    // Candidate lists remain opt-in through Tab, so ordinary typing never makes the input jump.
    fn refresh_hint(&mut self) {
        self.dismiss();
        let completion = repl::complete(&self.buffer());
        if completion.options.is_empty() {
            self.hint = completion.hint;
        }
    }

    fn remember(&mut self, line: String) {
        if line.is_empty() {
            return;
        }
        self.history.retain(|entry| entry != &line);
        self.history.push(line);
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
    }

    // arrow-up walks back through submitted lines and arrow-down returns to the line being written.
    fn recall_history(&mut self, back: bool) {
        if self.history.is_empty() {
            return;
        }
        let index = match (self.recall, back) {
            (None, true) => {
                self.draft = self.buffer();
                self.history.len().saturating_sub(1)
            }
            (None, false) => return,
            (Some(index), true) => index.saturating_sub(1),
            (Some(index), false) => index + 1,
        };
        if index >= self.history.len() {
            self.recall = None;
            let draft = std::mem::take(&mut self.draft);
            self.set(&draft);
            return;
        }
        self.recall = Some(index);
        let line = self.history[index].clone();
        self.set(&line);
    }

    fn set(&mut self, text: &str) {
        self.characters = text.chars().collect();
        self.cursor = self.characters.len();
        self.refresh_hint();
    }

    // tab completes the word under the caret when there is one answer, and lists the choices when
    // there are several — the same bargain a shell makes.
    fn complete(&mut self) {
        let buffer = self.buffer();
        let completion = repl::complete(&buffer);
        if completion.options.is_empty() {
            // Nothing to insert, but the catalog may still know what belongs here — a UUID, a
            // workflow name, a closed set the argument did not declare — and saying so is more use
            // than a silent Tab.
            self.menu.clear();
            self.hint = completion.hint;
            return;
        }
        self.hint = None;
        // the replaced word is measured in bytes by the completer and in characters here.
        let start = buffer[..completion.start].chars().count();
        if let [only] = completion.options.as_slice() {
            self.replace_word(start, &format!("{only} "));
            self.refresh_hint();
            return;
        }
        let shared = common_prefix(&completion.options);
        if shared.chars().count() > self.characters.len() - start {
            self.replace_word(start, &shared);
        }
        self.menu = completion.options;
    }

    fn replace_word(&mut self, start: usize, replacement: &str) {
        self.characters.truncate(start);
        self.characters.extend(replacement.chars());
        self.cursor = self.characters.len();
    }
}

fn common_prefix(values: &[String]) -> String {
    let Some((first, rest)) = values.split_first() else {
        return String::new();
    };
    let mut shared = first.clone();
    for value in rest {
        let length = shared
            .chars()
            .zip(value.chars())
            .take_while(|(left, right)| left == right)
            .count();
        shared.truncate(
            shared
                .char_indices()
                .nth(length)
                .map_or(shared.len(), |(at, _)| at),
        );
    }
    shared
}

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;
