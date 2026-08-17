//! the console's output scrollback.
//!
//! pure: it takes the bytes a command wrote, keeps them as lines, and answers "what is visible at
//! this scroll position". nothing here touches a terminal or a file descriptor, which is what lets
//! the scrolling rules be tested without either.
//!
//! the offset is measured from the *tail* rather than from the top, because the top is where lines
//! are discarded when the log is full: an offset counted from the front would slide the view every
//! time an old line fell off.

use std::collections::VecDeque;

/// how many lines of command output the console keeps.
pub(crate) const LINE_LIMIT: usize = 5_000;

/// how far the pane moves per wheel notch.
pub(crate) const WHEEL_ROWS: isize = 3;

/// how far the pane moves sideways per step, in columns.
pub(crate) const COLUMN_STEP: isize = 8;

/// the furthest right the pane can be scrolled; wide enough for any table worth reading.
const COLUMN_LIMIT: usize = 512;

/// tab stops, for output that indents with tabs rather than spaces.
const TAB_WIDTH: usize = 8;

/// a control sequence longer than this is malformed; it is dropped rather than accumulated.
const SEQUENCE_LIMIT: usize = 32;

/// what the output pane shows right now.
#[derive(Debug)]
pub(crate) struct Window<'a> {
    /// the visible lines, top to bottom.
    pub lines: Vec<&'a str>,
    /// one-based index of the first visible line, for the pane header.
    pub first: usize,
    /// how many lines are retained in total.
    pub total: usize,
    /// how many lines were discarded to stay under the limit.
    pub dropped: usize,
    /// true when the view is pinned to the newest output.
    pub following: bool,
    /// the first visible column.
    pub column: usize,
}

/// the retained output of a console session, and where the pane is looking.
#[derive(Debug)]
pub(crate) struct Transcript {
    lines: VecDeque<String>,
    /// the line being written, when the last chunk had no trailing newline.
    pending: String,
    /// how many rows are hidden below the view; zero follows the newest output.
    offset: usize,
    /// the first visible column, for output too wide for the pane.
    column: usize,
    /// lines discarded to stay under the limit, so the pane can say so.
    dropped: usize,
    limit: usize,
    scan: Scan,
    /// the parameter bytes of the control sequence being read.
    parameters: String,
}

/// where the reader is in a terminal control sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scan {
    Text,
    /// an escape has arrived; the next character says what kind of sequence this is.
    Escape,
    /// inside `ESC [ … <final>`.
    Csi,
    /// inside a string sequence, which runs to a bel or a string terminator.
    Terminated,
    /// an escape inside a string sequence, which may be the terminator's first half.
    TerminatedEscape,
    /// one byte to discard, for the two-character escapes.
    Skip,
}

impl Default for Transcript {
    fn default() -> Self {
        Self::with_limit(LINE_LIMIT)
    }
}

impl Transcript {
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            pending: String::new(),
            offset: 0,
            column: 0,
            dropped: 0,
            limit: limit.max(1),
            scan: Scan::Text,
            parameters: String::new(),
        }
    }

    /// how many rows the log holds, counting a half-written line as one.
    pub(crate) fn rows(&self) -> usize {
        self.lines.len() + usize::from(!self.pending.is_empty())
    }

    pub(crate) fn following(&self) -> bool {
        self.offset == 0
    }

    /// take what a command wrote.
    ///
    /// terminal control sequences are read and dropped rather than stored: the pane draws styled
    /// text of its own, and a stray `\x1b[…m` in the log would be printed literally. two of them do
    /// mean something here — a carriage return rewrites the current line, and an erase-display
    /// clears the log, which is what keeps `:clear` working through the capture.
    pub(crate) fn write(&mut self, chunk: &str) {
        for character in chunk.chars() {
            match self.scan {
                Scan::Text => self.text(character),
                Scan::Escape => self.escape(character),
                Scan::Csi => self.csi(character),
                Scan::Terminated => self.terminated(character),
                Scan::TerminatedEscape => {
                    self.scan = if character == '\\' {
                        Scan::Text
                    } else {
                        Scan::Terminated
                    };
                }
                Scan::Skip => self.scan = Scan::Text,
            }
        }
    }

    /// forget every retained line.
    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.pending.clear();
        self.offset = 0;
        self.dropped = 0;
    }

    /// keep the offset inside what a pane of `height` rows can show.
    ///
    /// the pane's height is only known at draw time, and it changes when the terminal is resized or
    /// the input grows a line, so the offset is clamped there rather than at every scroll.
    pub(crate) fn clamp(&mut self, height: usize) {
        self.offset = self.offset.min(self.ceiling(height));
    }

    /// scroll by rows: negative moves back through the output, positive returns toward the newest.
    pub(crate) fn scroll(&mut self, rows: isize, height: usize) {
        let ceiling = self.ceiling(height) as isize;
        self.offset = (self.offset as isize - rows).clamp(0, ceiling) as usize;
    }

    /// scroll by whole panes.
    pub(crate) fn scroll_pages(&mut self, pages: isize, height: usize) {
        // one row of overlap, so the line a page break lands on is readable in both pages.
        let page = height.saturating_sub(1).max(1) as isize;
        self.scroll(pages * page, height);
    }

    pub(crate) fn scroll_columns(&mut self, columns: isize) {
        self.column = (self.column as isize + columns).clamp(0, COLUMN_LIMIT as isize) as usize;
    }

    /// pin the view back to the newest output.
    pub(crate) fn follow(&mut self) {
        self.offset = 0;
    }

    /// jump to the oldest retained line.
    pub(crate) fn rewind(&mut self, height: usize) {
        self.offset = self.ceiling(height);
    }

    /// the visible rows for a pane of `height` rows.
    pub(crate) fn view(&self, height: usize) -> Window<'_> {
        let rows = self.rows();
        let end = rows.saturating_sub(self.offset);
        let start = end.saturating_sub(height);
        Window {
            lines: (start..end).filter_map(|index| self.row(index)).collect(),
            first: if rows == 0 { 0 } else { start + 1 },
            total: rows,
            dropped: self.dropped,
            following: self.following(),
            column: self.column,
        }
    }

    /// every retained line, oldest first — what the console replays to the terminal on the way out.
    pub(crate) fn replay(&self) -> Vec<&str> {
        (0..self.rows())
            .filter_map(|index| self.row(index))
            .collect()
    }

    fn row(&self, index: usize) -> Option<&str> {
        match self.lines.get(index) {
            Some(line) => Some(line.as_str()),
            None if index == self.lines.len() && !self.pending.is_empty() => {
                Some(self.pending.as_str())
            }
            None => None,
        }
    }

    // the furthest back the view can go: every row but the paneful being shown.
    fn ceiling(&self, height: usize) -> usize {
        self.rows().saturating_sub(height.max(1))
    }

    fn text(&mut self, character: char) {
        match character {
            '\n' => self.newline(),
            // a carriage return rewrites the line in place, the way a progress counter does.
            '\r' => self.pending.clear(),
            '\t' => {
                let stop = TAB_WIDTH - self.pending.chars().count() % TAB_WIDTH;
                self.pending.push_str(&" ".repeat(stop));
            }
            '\u{8}' => {
                self.pending.pop();
            }
            '\u{1b}' => self.scan = Scan::Escape,
            // remaining control characters have no meaning in a log line.
            character if character.is_control() => {}
            character => self.pending.push(character),
        }
    }

    fn escape(&mut self, character: char) {
        self.parameters.clear();
        self.scan = match character {
            '[' => Scan::Csi,
            // osc, dcs, sos, pm, apc: a string that runs to its own terminator.
            ']' | 'P' | 'X' | '^' | '_' => Scan::Terminated,
            // a character-set selection takes one more byte.
            '(' | ')' | '*' | '+' | '%' | '#' => Scan::Skip,
            _ => Scan::Text,
        };
    }

    fn csi(&mut self, character: char) {
        // parameter and intermediate bytes come first; the final byte says what the sequence does.
        if ('\u{20}'..'\u{40}').contains(&character) {
            if self.parameters.chars().count() < SEQUENCE_LIMIT {
                self.parameters.push(character);
            }
            return;
        }
        // an erase-display of everything is the one sequence the log acts on.
        if character == 'J' && matches!(self.parameters.as_str(), "2" | "3") {
            self.clear();
        }
        self.scan = Scan::Text;
    }

    fn terminated(&mut self, character: char) {
        self.scan = match character {
            '\u{7}' => Scan::Text,
            '\u{1b}' => Scan::TerminatedEscape,
            _ => Scan::Terminated,
        };
    }

    fn newline(&mut self) {
        self.lines.push_back(std::mem::take(&mut self.pending));
        if self.lines.len() > self.limit {
            self.lines.pop_front();
            self.dropped += 1;
        }
        // a view that has been scrolled back stays on the lines it was showing; only a following
        // view moves with the output.
        if self.offset > 0 {
            self.offset += 1;
        }
    }
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
