//! The console's output scrollback.
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
pub(crate) const LINE_LIMIT: usize = 10_000;
const BYTE_LIMIT: usize = 8 * 1024 * 1024;

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

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;

mod window;
pub(crate) use window::Window;

mod transcript;
pub(crate) use transcript::Transcript;
