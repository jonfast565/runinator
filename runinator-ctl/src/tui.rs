//! the console's terminal ui.
//!
//! the prompt is an *inline* ratatui viewport pinned to the bottom of the terminal: a status line,
//! the input, a completion menu, and a key legend. everything a command prints still goes to stdout
//! and scrolls above it, which is the point — the command modules keep their plain `println!`, and
//! the ui is suspended for the duration of a command rather than trying to capture its output.
//!
//! `--plain` keeps the older reedline prompt for terminals (and pipes) where this is not wanted.

use std::io::{self, Stdout, Write};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::crossterm::{cursor, execute, terminal};
use ratatui::{TerminalOptions, Viewport};

use crate::commands::Result;

pub(crate) mod editor;
pub(crate) mod render;

use editor::{Outcome, PromptEditor};
use render::{PromptView, VIEWPORT_ROWS};

/// what one turn of the prompt produced.
pub(crate) enum Submission {
    Line(String),
    Exit,
}

/// the console prompt, owning the terminal while it is being drawn.
pub(crate) struct Prompt {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    editor: PromptEditor,
    session: String,
    api_base_url: String,
    note: Option<String>,
    raw: bool,
}

impl Prompt {
    pub(crate) fn new(session: String, api_base_url: String) -> Result<Self> {
        let mut prompt = Self {
            terminal: inline_terminal()?,
            editor: PromptEditor::default(),
            session,
            api_base_url,
            note: None,
            raw: false,
        };
        prompt.enter_raw()?;
        Ok(prompt)
    }

    /// the session shown in the status line, which `:use` and `:new` change.
    pub(crate) fn set_session(&mut self, session: String) {
        self.session = session;
    }

    /// the message shown under the input until the next line is typed.
    pub(crate) fn set_note(&mut self, note: Option<String>) {
        self.note = note;
    }

    /// read one line, blocking until it is submitted or the console is closed.
    pub(crate) fn read_line(&mut self) -> Result<Submission> {
        loop {
            self.draw("ready")?;
            // polling rather than blocking on `read` keeps a terminal resize from being noticed only
            // after the next keystroke.
            if !event::poll(Duration::from_millis(200))? {
                continue;
            }
            match event::read()? {
                Event::Key(key) => match self.editor.handle(key) {
                    Outcome::Pending => {}
                    Outcome::Submit(line) => {
                        self.note = None;
                        return Ok(Submission::Line(line));
                    }
                    Outcome::Exit => return Ok(Submission::Exit),
                    Outcome::ClearScreen => self.wipe()?,
                },
                Event::Resize(_, _) => {
                    self.terminal.autoresize()?;
                }
                _ => {}
            }
        }
    }

    /// hand the terminal back so a command can print to it normally.
    ///
    /// raw mode goes with it: a command that runs for a while has to stay interruptible with Ctrl+C,
    /// and in raw mode that keystroke would never become a signal.
    pub(crate) fn suspend(&mut self) -> Result<()> {
        // where the viewport starts, taken before it is cleared: that row is the first line the
        // command may print on.
        let top = self.terminal.get_frame().area().y;
        self.terminal.clear()?;
        self.leave_raw()?;
        // `Terminal::clear` restores the cursor to wherever the caret was — several columns into
        // the input band, plus however much had been typed. printing from there would indent the
        // echoed line and everything the command writes after it, so output starts at the left
        // edge of the space the viewport just gave back.
        execute!(io::stdout(), cursor::MoveTo(0, top))?;
        Ok(())
    }

    /// take the terminal back after a command has printed.
    pub(crate) fn resume(&mut self) -> Result<()> {
        // the viewport always starts at column 0, so a command whose last write had no trailing
        // newline would have the prompt drawn over it. one newline puts the anchor on a clean row.
        //
        // the flush is what makes the question answerable: stdout is line-buffered, so a `print!`
        // with no newline is still in the buffer and the terminal's cursor has not moved for it.
        io::stdout().flush()?;
        if cursor::position().is_ok_and(|(column, _)| column > 0) {
            println!();
        }
        // the viewport is anchored where it was created, and the command just scrolled the screen,
        // so it is re-anchored rather than redrawn in place.
        self.terminal = inline_terminal()?;
        self.enter_raw()
    }

    /// echo a submitted line into the scrollback, so the transcript reads like a session.
    pub(crate) fn echo(&mut self, line: &str) {
        let command = line.trim_start().starts_with(':');
        let sigil = if command { ":" } else { "›" };
        for (index, text) in line.lines().enumerate() {
            let marker = if index == 0 { sigil } else { "·" };
            println!("{marker} {text}");
        }
    }

    /// the lines typed so far, which the caller persists between sessions.
    pub(crate) fn history(&self) -> Vec<String> {
        self.editor.history().to_vec()
    }

    pub(crate) fn with_history(mut self, history: Vec<String>) -> Self {
        self.editor = PromptEditor::new(history);
        self
    }

    fn draw(&mut self, state: &str) -> Result<()> {
        let buffer = self.editor.buffer();
        let view = PromptView {
            session: &self.session,
            api_base_url: &self.api_base_url,
            state,
            buffer: &buffer,
            caret: self.editor.caret(),
            menu: &self.editor.menu,
            hint: self.editor.hint.as_deref(),
            note: self.note.as_deref(),
        };
        self.terminal.draw(|frame| render::draw(frame, &view))?;
        Ok(())
    }

    // ctrl+l: scroll the screen away and re-anchor, which is what a shell's clear does.
    fn wipe(&mut self) -> Result<()> {
        self.terminal.clear()?;
        self.leave_raw()?;
        execute!(
            io::stdout(),
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(0, 0)
        )?;
        self.terminal = inline_terminal()?;
        self.enter_raw()
    }

    fn enter_raw(&mut self) -> Result<()> {
        if !self.raw {
            enable_raw_mode()?;
            self.raw = true;
        }
        Ok(())
    }

    fn leave_raw(&mut self) -> Result<()> {
        if self.raw {
            disable_raw_mode()?;
            self.raw = false;
        }
        let mut stdout = io::stdout();
        execute!(stdout, cursor::Show)?;
        stdout.flush()?;
        Ok(())
    }
}

impl Drop for Prompt {
    fn drop(&mut self) {
        // the terminal must come back even on an error path: a console left in raw mode is a shell
        // that no longer echoes what is typed into it. suspending is exactly that plus putting the
        // cursor back at the left edge, so the shell prompt is not indented by the console's caret.
        let _ = self.suspend();
    }
}

fn inline_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    Ok(Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(VIEWPORT_ROWS),
        },
    )?)
}
