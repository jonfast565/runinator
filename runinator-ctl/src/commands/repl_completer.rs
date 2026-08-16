//! reedline's view of the console's completion.
//!
//! the candidates themselves come from `repl::complete`, which the tui prompt also uses — the two
//! front ends must offer the same words or `Tab` would mean something different depending on which
//! one you opened.

use reedline::{Completer, CompletionResult, Span, Suggestion};

use super::repl;

/// completes console verbs, command-line verbs, their subcommands, and their long flags.
pub(super) struct ReplCompleter;

impl Completer for ReplCompleter {
    fn complete(&mut self, line: &str, position: usize) -> CompletionResult {
        CompletionResult::fresh(suggestions(&line[..position.min(line.len())], position))
    }
}

fn suggestions(line: &str, position: usize) -> Vec<Suggestion> {
    let completion = repl::complete(line);
    let span = Span::new(completion.start, position);
    completion
        .options
        .into_iter()
        .map(|value| Suggestion {
            value,
            display_override: None,
            description: None,
            style: None,
            extra: None,
            span,
            append_whitespace: true,
            match_indices: None,
        })
        .collect()
}

#[cfg(test)]
#[path = "repl_completer_tests.rs"]
mod tests;
