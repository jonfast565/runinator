#[allow(unused_imports)]
use super::*;

/// one console verb: the words that select it, how it is called, and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
    pub path: Vec<String>,
    pub usage: String,
    pub summary: String,
    /// the console answers this itself; everything else is dispatched as a command line.
    pub console_local: bool,
}

impl CommandEntry {
    /// the path as one word, which is how `:help` names a command.
    pub fn name(&self) -> String {
        self.path.join(" ")
    }
}
