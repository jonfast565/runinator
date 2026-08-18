//! the console's terminal ui.
//!
//! the console owns the screen: a status line, a scrollable pane holding everything commands have
//! printed, the input, a completion menu, and a key legend. the pane can hold that output because
//! the process's own stdout and stderr are redirected into it (`capture`), which is what lets the
//! command modules keep their plain `println!` while the console gets a log it can page through.
//!
//! two consequences of owning the screen are deliberate. the interface draws on a *duplicate* of the
//! original stdout, never on `io::stdout()`, which by then is the pipe feeding the pane. and on the
//! way out the transcript is replayed to the terminal, so quitting the console leaves the session's
//! output in the shell's scrollback exactly as it would have been without it.
//!
//! `--plain` keeps the older reedline prompt, and so does any terminal where the capture cannot be
//! installed.

use std::future::Future;
use std::io::{self, Write};
use std::sync::MutexGuard;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::crossterm::{cursor, execute};
use ratatui::layout::Position;

use crate::commands::Result;

pub(crate) mod capture;
pub(crate) mod editor;
pub(crate) mod render;
pub(crate) mod transcript;

use capture::{Capture, Screen, Shared};
use editor::{Outcome, PromptEditor};
use render::{Bands, Pane, PromptView};
use transcript::{COLUMN_STEP, LINE_LIMIT, Transcript, WHEEL_ROWS};

/// how long a keystroke may wait before the pane is redrawn with whatever a command has printed.
const TICK: Duration = Duration::from_millis(100);

/// what one turn of the prompt produced.
pub(crate) enum Submission {
    Line(String),
    Exit,
}

/// the console, owning the terminal and the captured output while it is open.
pub(crate) struct Prompt {
    terminal: Terminal<CrosstermBackend<Screen>>,
    editor: PromptEditor,
    transcript: Shared,
    /// dropped before the transcript is replayed, which is what puts stdout back first.
    capture: Option<Capture>,
    /// the last frame's layout, for deciding which pane the pointer is over.
    bands: Option<Bands>,
    /// the input pane's scroll position while it is being scrolled by hand.
    input_scroll: Option<u16>,
    session: String,
    api_base_url: String,
    note: Option<String>,
    raw: bool,
    screen: bool,
}

impl Prompt {
    pub(crate) fn new(session: String, api_base_url: String) -> Result<Self> {
        let (capture, screen, transcript) = Capture::install(LINE_LIMIT)?;
        let mut prompt = Self {
            terminal: Terminal::new(CrosstermBackend::new(screen))?,
            editor: PromptEditor::default(),
            transcript,
            capture: Some(capture),
            bands: None,
            input_scroll: None,
            session,
            api_base_url,
            note: None,
            raw: false,
            screen: false,
        };
        prompt.enter()?;
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
            // polling rather than blocking on `read` is what lets output a command is still writing
            // reach the pane, and a resize be noticed before the next keystroke.
            if !event::poll(TICK)? {
                continue;
            }
            match event::read()? {
                Event::Key(key) => {
                    if self.scrolled(key) {
                        continue;
                    }
                    // typing puts the input pane back under the caret; only the wheel moves it away.
                    self.input_scroll = None;
                    match self.editor.handle(key) {
                        Outcome::Pending => {}
                        Outcome::Submit(line) => {
                            self.note = None;
                            return Ok(Submission::Line(line));
                        }
                        Outcome::Exit => return Ok(Submission::Exit),
                        Outcome::ClearScreen => self.log().clear(),
                    }
                }
                Event::Mouse(mouse) => self.wheeled(mouse),
                Event::Resize(_, _) => self.terminal.autoresize()?,
                _ => {}
            }
        }
    }

    /// run a command, keeping the console drawn while it works.
    ///
    /// `None` means Ctrl+C: the command's future is dropped, which cancels whatever it was waiting
    /// on. output keeps arriving in the pane throughout, and the pane stays scrollable, so a long
    /// run can be read while it is still going.
    pub(crate) async fn run<T>(&mut self, task: impl Future<Output = T>) -> Result<Option<T>> {
        let mut task = std::pin::pin!(task);
        loop {
            tokio::select! {
                biased;
                finished = &mut task => {
                    // whatever the command wrote last is still in the pipe; one more turn of the
                    // reader thread puts it in the pane before the prompt returns.
                    tokio::time::sleep(TICK).await;
                    self.draw("ready")?;
                    return Ok(Some(finished));
                }
                _ = tokio::time::sleep(TICK) => {
                    if self.interrupted()? {
                        return Ok(None);
                    }
                    self.draw("running")?;
                }
            }
        }
    }

    /// echo a submitted line into the transcript, so it reads like a session.
    ///
    /// this prints, and printing is captured, so the echo lands in the pane ahead of whatever the
    /// command writes next — in order, since both travel the same pipe.
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

    // scroll keys are read before the editor sees them, so the editor stays purely about the line
    // and history recall keeps every key it had. the keyboard always scrolls the output pane: the
    // input's own scrolling is the wheel's business, since the caret is what moves it otherwise.
    fn scrolled(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        let shift = key.modifiers == KeyModifiers::SHIFT;
        let height = self.pane_height();
        let mut log = self.log();
        match key.code {
            KeyCode::PageUp => log.scroll_pages(-1, height),
            KeyCode::PageDown => log.scroll_pages(1, height),
            KeyCode::Up if shift => log.scroll(-1, height),
            KeyCode::Down if shift => log.scroll(1, height),
            KeyCode::Home if shift => log.rewind(height),
            KeyCode::End if shift => log.follow(),
            KeyCode::Left if shift => log.scroll_columns(-COLUMN_STEP),
            KeyCode::Right if shift => log.scroll_columns(COLUMN_STEP),
            _ => return false,
        }
        true
    }

    // the wheel scrolls whichever pane the pointer is over, which is the only reading of it that
    // needs no explaining.
    fn wheeled(&mut self, mouse: MouseEvent) {
        let Some(bands) = self.bands else {
            return;
        };
        let pane = bands.pane_at(Position::new(mouse.column, mouse.row));
        let height = self.pane_height();
        match (mouse.kind, pane) {
            (MouseEventKind::ScrollUp, Some(Pane::Input)) => self.scroll_input(-1),
            (MouseEventKind::ScrollDown, Some(Pane::Input)) => self.scroll_input(1),
            // anywhere but the input — including the status line and the legend — means the output,
            // which is the pane a wheel is nearly always aimed at.
            (MouseEventKind::ScrollUp, _) => self.log().scroll(-WHEEL_ROWS, height),
            (MouseEventKind::ScrollDown, _) => self.log().scroll(WHEEL_ROWS, height),
            (MouseEventKind::ScrollLeft, _) => self.log().scroll_columns(-COLUMN_STEP),
            (MouseEventKind::ScrollRight, _) => self.log().scroll_columns(COLUMN_STEP),
            _ => {}
        }
    }

    // the input pane holds four rows at most, so a long WDL cell scrolls inside it.
    fn scroll_input(&mut self, rows: i16) {
        let buffer = self.editor.buffer();
        let lines = buffer.split('\n').count() as i32;
        let text = render::input_rows(&buffer).saturating_sub(1) as i32;
        let ceiling = (lines - text).max(0);
        let current = i32::from(self.input_scroll.unwrap_or(ceiling as u16));
        self.input_scroll = Some((current + i32::from(rows)).clamp(0, ceiling) as u16);
    }

    // drain whatever has arrived without waiting, so a command's own work is not paused for it.
    // Ctrl+C is the only key that means anything mid-command; the rest would be typing into a line
    // that is not being edited yet.
    fn interrupted(&mut self) -> Result<bool> {
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(key)
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                        && key.kind != KeyEventKind::Release =>
                {
                    return Ok(true);
                }
                Event::Key(key) => {
                    self.scrolled(key);
                }
                Event::Mouse(mouse) => self.wheeled(mouse),
                Event::Resize(_, _) => self.terminal.autoresize()?,
                _ => {}
            }
        }
        Ok(false)
    }

    fn draw(&mut self, state: &str) -> Result<()> {
        let buffer = self.editor.buffer();
        let footer = self.note.is_some() || self.editor.hint.is_some();
        let area = self.terminal.get_frame().area();
        let height = render::bands(area, &buffer, self.editor.menu.len(), footer)
            .output_lines()
            .height as usize;

        // the offset is clamped against the height the pane actually has, which the layout only
        // knows here; a resize or a taller input can otherwise leave it past the end.
        let mut transcript = lock(&self.transcript);
        transcript.clamp(height);
        let window = transcript.view(height);
        let view = PromptView {
            session: &self.session,
            api_base_url: &self.api_base_url,
            state,
            output: &window,
            buffer: &buffer,
            caret: self.editor.caret(),
            input_scroll: self.input_scroll,
            menu: &self.editor.menu,
            hint: self.editor.hint.as_deref(),
            note: self.note.as_deref(),
        };
        let mut bands = None;
        self.terminal
            .draw(|frame| bands = Some(render::draw(frame, &view)))?;
        self.bands = bands;
        Ok(())
    }

    // how many rows of output are on screen, which every page and clamp is measured in.
    fn pane_height(&self) -> usize {
        self.bands
            .map(|bands| bands.output_lines().height as usize)
            .unwrap_or(1)
            .max(1)
    }

    fn log(&self) -> MutexGuard<'_, Transcript> {
        lock(&self.transcript)
    }

    fn enter(&mut self) -> Result<()> {
        enable_raw_mode()?;
        self.raw = true;
        // the screen is cleared with crossterm's own command rather than `Terminal::clear`, which
        // asks the terminal where the cursor is and waits for the reply. nothing here needs that
        // answer — the alternate screen starts blank and the first draw paints all of it — and a
        // terminal slow to reply would drop the console to the plain prompt for no reason.
        execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture,
            Clear(ClearType::All)
        )?;
        self.screen = true;
        Ok(())
    }

    // give the terminal back: the alternate screen, the mouse, raw mode, and the cursor.
    fn leave(&mut self) {
        if self.screen {
            let _ = execute!(
                self.terminal.backend_mut(),
                DisableMouseCapture,
                LeaveAlternateScreen,
                cursor::Show
            );
            self.screen = false;
        }
        if self.raw {
            let _ = disable_raw_mode();
            self.raw = false;
        }
        let _ = self.terminal.backend_mut().flush();
    }

    // put the session's output back in the terminal's own scrollback, so closing the console leaves
    // behind what the shell would have had if the console had never taken the screen.
    fn replay(&self) {
        let transcript = lock(&self.transcript);
        let lines = transcript.replay();
        if lines.is_empty() {
            return;
        }
        let mut stdout = io::stdout().lock();
        for line in lines {
            let _ = writeln!(stdout, "{line}");
        }
        let _ = stdout.flush();
    }
}

impl Drop for Prompt {
    fn drop(&mut self) {
        // order matters: the screen goes back first, then stdout stops being a pipe, and only then
        // is there a terminal to replay the transcript to.
        self.leave();
        if let Some(mut capture) = self.capture.take() {
            capture.restore();
        }
        self.replay();
    }
}

// a panic on the reader thread must not take the console with it: the log it was holding is still
// the log, and losing the terminal restore would be far worse than a torn line.
fn lock(transcript: &Shared) -> MutexGuard<'_, Transcript> {
    transcript
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
