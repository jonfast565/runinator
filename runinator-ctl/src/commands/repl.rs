//! Native-console adapter over the portable `runinatorctl` command language.
//!
//! Parsing, clap validation, and completion live in `runinator-ctl-core`, which is also compiled
//! to WASM for Command Center. Only terminal-width-aware help rendering remains native here.

use runinator_ctl_core::console::{self as core, catalog};

use crate::commands::{Result, err};
use crate::output;

pub(crate) use core::Completion;
pub(super) use core::{Arguments, ReplCommand, Token};

pub(super) fn scan(line: &str) -> Result<Vec<Token>> {
    core::scan(line).map_err(err)
}

pub(super) fn tokenize(line: &str) -> Result<Vec<String>> {
    core::tokenize(line).map_err(err)
}

pub(super) fn parse_arguments(tokens: &[Token], booleans: &[&str]) -> Arguments {
    core::parse_arguments(tokens, booleans)
}

pub(super) struct MetaMatch {
    pub command: &'static core::MetaCommand,
    pub arguments: Arguments,
}

pub(super) fn match_meta(tokens: &[Token]) -> Option<MetaMatch> {
    let words: Vec<String> = tokens.iter().map(|token| token.text.clone()).collect();
    let command = catalog::match_meta(&words)?;
    Some(MetaMatch {
        arguments: parse_arguments(&tokens[command.path.len()..], command.booleans),
        command,
    })
}

pub(super) fn parse(tokens: &[String]) -> Result<ReplCommand> {
    core::parse(tokens).map_err(err)
}

pub(super) fn unknown_command(word: &str) -> String {
    core::unknown_command(word)
}

pub(super) fn help(topic: Option<&str>) -> Result<String> {
    let entries = core::catalog();
    let Some(topic) = topic.map(|topic| topic.trim().trim_start_matches(':')) else {
        let names: Vec<String> = entries
            .iter()
            .map(|entry| format!(":{}", entry.name()))
            .collect();
        let width = column_width(names.iter().map(String::as_str));
        let rows: Vec<Vec<String>> = entries
            .iter()
            .zip(names)
            .map(|(entry, name)| vec![name, output::truncate(&entry.summary, summary_width(width))])
            .collect();
        let mut text = String::from(
            "a bare line is REXRAP; a `:` line is a command. `:help <command>` for arguments.\n\
             every command also takes `--json` to print the raw payload.\n\n",
        );
        text.push_str(&output::table(&["command", "what it does"], &rows));
        return Ok(text);
    };

    let matches: Vec<&core::CommandEntry> = entries
        .iter()
        .filter(|entry| entry.name().starts_with(topic))
        .collect();
    if matches.is_empty() {
        return Err(err(unknown_command(topic)));
    }
    if let [entry] = matches.as_slice() {
        let mut text = format!("{}\n{}\n", entry.usage, entry.summary);
        let arguments = catalog::arguments(&entry.path);
        if !arguments.is_empty() {
            let rows: Vec<Vec<String>> = arguments
                .into_iter()
                .map(|(label, help)| vec![label, help])
                .collect();
            text.push('\n');
            text.push_str(&output::table(&["argument", "what it is"], &rows));
        }
        return Ok(text);
    }

    let width = column_width(matches.iter().map(|entry| entry.usage.as_str()));
    let rows: Vec<Vec<String>> = matches
        .iter()
        .map(|entry| {
            vec![
                output::truncate(&entry.usage, width),
                output::truncate(&entry.summary, summary_width(width)),
            ]
        })
        .collect();
    Ok(output::table(&["usage", "what it does"], &rows))
}

fn column_width<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    let widest = values.map(|value| value.chars().count()).max().unwrap_or(0);
    widest.min(output::terminal_width() / 2).max(12)
}

fn summary_width(first: usize) -> usize {
    output::terminal_width().saturating_sub(first + 3).max(24)
}

pub(crate) fn is_submittable(source: &str) -> bool {
    core::is_submittable(source)
}

pub(crate) fn complete(line: &str) -> Completion {
    core::complete(line)
}

pub(super) fn command_names() -> Vec<String> {
    core::command_names()
}

#[cfg(test)]
#[path = "repl_tests.rs"]
mod tests;
