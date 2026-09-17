#[allow(unused_imports)]
use super::*;

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
