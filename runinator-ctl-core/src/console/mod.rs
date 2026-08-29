//! The portable command-console language used by native and browser front ends.

pub mod catalog;

use std::collections::BTreeMap;

use clap::{CommandFactory, Parser};
use serde::Serialize;

use crate::cli::Commands;

pub use catalog::{CommandEntry, MetaCommand};

#[derive(Debug, Parser)]
#[command(
    name = "runinatorctl",
    no_binary_name = true,
    disable_help_subcommand = true,
    about = "Every runinatorctl command, prefixed with `:` inside the console"
)]
pub struct ReplCommand {
    /// Print this command's output as json.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub raw: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Arguments {
    pub args: Vec<String>,
    pub raw_args: Vec<String>,
    pub flags: BTreeMap<String, Vec<String>>,
    pub switches: Vec<String>,
}

impl Arguments {
    pub fn raw_after(&self, word: &str) -> Option<String> {
        let at = self.args.iter().position(|value| value == word)?;
        Some(self.raw_args[at + 1..].join(" "))
    }

    pub fn arg(&self, index: usize) -> Option<&str> {
        self.args.get(index).map(String::as_str)
    }

    pub fn required(&self, index: usize, name: &str) -> Result<&str, String> {
        self.arg(index)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{name} is required"))
    }

    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .get(name)
            .and_then(|values| values.last())
            .map(String::as_str)
    }

    pub fn flag_list(&self, name: &str) -> &[String] {
        self.flags.get(name).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn is_set(&self, name: &str) -> bool {
        self.switches.iter().any(|candidate| candidate == name) || self.flags.contains_key(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Completion {
    pub start: usize,
    pub options: Vec<String>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArgumentSpec {
    pub label: String,
    pub help: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandSpec {
    pub path: Vec<String>,
    pub usage: String,
    pub summary: String,
    pub console_local: bool,
    pub arguments: Vec<ArgumentSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParsedLine {
    Empty,
    Command {
        path: Vec<String>,
        args: Vec<String>,
        raw_args: Vec<String>,
        flags: BTreeMap<String, Vec<String>>,
        switches: Vec<String>,
        json: bool,
        console_local: bool,
    },
}

pub fn catalog() -> &'static [CommandEntry] {
    catalog::catalog()
}

pub fn command_specs() -> Vec<CommandSpec> {
    catalog()
        .iter()
        .map(|entry| CommandSpec {
            path: entry.path.clone(),
            usage: entry.usage.clone(),
            summary: entry.summary.clone(),
            console_local: entry.console_local,
            arguments: catalog::arguments(&entry.path)
                .into_iter()
                .map(|(label, help)| ArgumentSpec { label, help })
                .collect(),
        })
        .collect()
}

pub fn scan(line: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut start = None;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for (at, character) in line.char_indices() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            start.get_or_insert(at);
            continue;
        }
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => current.push(character),
            None if matches!(character, '\'' | '"') => {
                quote = Some(character);
                start.get_or_insert(at);
            }
            None if character.is_whitespace() => {
                if let Some(from) = start.take() {
                    tokens.push(Token {
                        text: std::mem::take(&mut current),
                        raw: line[from..at].to_string(),
                    });
                }
            }
            None => {
                current.push(character);
                start.get_or_insert(at);
            }
        }
    }
    if quote.is_some() {
        return Err("unterminated quote".to_string());
    }
    if escaped {
        return Err("line ends with a dangling backslash".to_string());
    }
    if let Some(from) = start {
        tokens.push(Token {
            text: current,
            raw: line[from..].to_string(),
        });
    }
    Ok(tokens)
}

pub fn tokenize(line: &str) -> Result<Vec<String>, String> {
    Ok(scan(line)?.into_iter().map(|token| token.text).collect())
}

pub fn parse_arguments(tokens: &[Token], booleans: &[&str]) -> Arguments {
    let mut parsed = Arguments::default();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index].text;
        index += 1;
        let Some(body) = token.strip_prefix("--") else {
            parsed.args.push(token.clone());
            parsed.raw_args.push(tokens[index - 1].raw.clone());
            continue;
        };
        if body.is_empty() {
            continue;
        }
        if let Some((name, value)) = body.split_once('=') {
            parsed
                .flags
                .entry(name.to_string())
                .or_default()
                .push(value.to_string());
            continue;
        }
        let next = tokens.get(index);
        let boolean = body == "json"
            || booleans.contains(&body)
            || next.is_none()
            || next.is_some_and(|token| token.text.starts_with("--"));
        if boolean {
            if !parsed.switches.iter().any(|candidate| candidate == body) {
                parsed.switches.push(body.to_string());
            }
            continue;
        }
        parsed
            .flags
            .entry(body.to_string())
            .or_default()
            .push(tokens[index].text.clone());
        index += 1;
    }
    parsed
}

pub fn parse(tokens: &[String]) -> Result<ReplCommand, String> {
    ReplCommand::try_parse_from(tokens).map_err(|error| error.render().to_string())
}

pub fn parse_line(line: &str) -> Result<ParsedLine, String> {
    let body = line.trim().trim_start_matches(':');
    let tokens = scan(body)?;
    if tokens.is_empty() {
        return Ok(ParsedLine::Empty);
    }
    let words: Vec<String> = tokens.iter().map(|token| token.text.clone()).collect();
    let Some(entry) = match_entry(&words) else {
        return Err(unknown_command(&words[0]));
    };
    if !entry.console_local {
        parse(&words)?;
    }
    let booleans: Vec<String> = if let Some(meta) = catalog::match_meta(&words) {
        meta.booleans
            .iter()
            .map(|value| (*value).to_string())
            .collect()
    } else {
        catalog::flag_names(&entry.path)
            .into_iter()
            .filter(|flag| !catalog::flag_takes_value(&entry.path, flag))
            .map(|flag| flag.trim_start_matches('-').to_string())
            .collect()
    };
    let boolean_refs: Vec<&str> = booleans.iter().map(String::as_str).collect();
    let arguments = parse_arguments(&tokens[entry.path.len()..], &boolean_refs);
    let json = arguments.is_set("json");
    Ok(ParsedLine::Command {
        path: entry.path.clone(),
        args: arguments.args,
        raw_args: arguments.raw_args,
        flags: arguments.flags,
        switches: arguments.switches,
        json,
        console_local: entry.console_local,
    })
}

fn match_entry(tokens: &[String]) -> Option<&'static CommandEntry> {
    catalog()
        .iter()
        .filter(|entry| {
            entry.path.len() <= tokens.len()
                && entry
                    .path
                    .iter()
                    .enumerate()
                    .all(|(index, word)| tokens[index] == *word)
        })
        .max_by_key(|entry| entry.path.len())
}

pub fn unknown_command(word: &str) -> String {
    let mut message = format!("unknown console command '{word}'");
    if let Some(nearest) = nearest_command(word) {
        message.push_str(&format!("; did you mean ':{nearest}'?"));
    }
    message.push_str(" try :help");
    message
}

fn nearest_command(word: &str) -> Option<String> {
    let limit = 1 + word.chars().count() / 3;
    first_words()
        .into_iter()
        .map(|candidate| (edit_distance(word, &candidate), candidate))
        .filter(|(distance, _)| *distance <= limit)
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(|(_, candidate)| candidate)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (row, from) in left.chars().enumerate() {
        current[0] = row + 1;
        for (column, to) in right.iter().enumerate() {
            let cost = usize::from(from != *to);
            current[column + 1] = (previous[column] + cost)
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

pub fn is_submittable(source: &str) -> bool {
    let trimmed = source.trim();
    !trimmed.is_empty()
        && (trimmed.starts_with(':') || (is_balanced(source) && !source.trim_end().ends_with('\\')))
}

pub fn is_balanced(source: &str) -> bool {
    let mut stack = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    for character in source.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            if character == open {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            continue;
        }
        match character {
            '{' | '[' | '(' => stack.push(character),
            '}' if stack.last() == Some(&'{') => {
                stack.pop();
            }
            ']' if stack.last() == Some(&'[') => {
                stack.pop();
            }
            ')' if stack.last() == Some(&'(') => {
                stack.pop();
            }
            _ => {}
        }
    }
    quote.is_none() && stack.is_empty()
}

pub fn complete(line: &str) -> Completion {
    let trimmed = line.trim_start();
    if !trimmed.starts_with(':') {
        return Completion {
            start: line.len(),
            options: Vec::new(),
            hint: None,
        };
    }
    let body_start = line.len() - trimmed.len() + 1;
    let body = &line[body_start..];
    let (prefix, start) = match body.rfind(char::is_whitespace) {
        Some(at) => (&body[at + 1..], body_start + at + 1),
        None => (body, body_start),
    };
    let typed = tokenize(&body[..start - body_start]).unwrap_or_default();
    let (options, hint) = candidates(&typed, prefix);
    Completion {
        start,
        options: matching(options, prefix),
        hint,
    }
}

fn candidates(typed: &[String], prefix: &str) -> (Vec<String>, Option<String>) {
    if let Some(flag) = pending_flag(typed) {
        let path = command_path(typed);
        let values = catalog::flag_values(&path, &flag);
        if !values.is_empty() {
            return (values, None);
        }
        if !prefix.starts_with("--") {
            return (Vec::new(), catalog::flag_hint(&path, &flag).map(describe));
        }
    }
    if prefix.starts_with("--") {
        return (flag_names(typed), None);
    }
    let mut words = Vec::new();
    for entry in catalog() {
        let aligns = entry
            .path
            .iter()
            .take(typed.len())
            .enumerate()
            .all(|(index, word)| typed[index] == *word);
        if aligns && let Some(next) = entry.path.get(typed.len()) {
            words.push(next.clone());
        }
    }
    if !words.is_empty() {
        return (words, None);
    }
    let path = command_path(typed);
    let position = typed.len().saturating_sub(path.len());
    let values = catalog::positional_values(&path, position);
    if !values.is_empty() {
        return (values, None);
    }
    (Vec::new(), value_hint(typed, &path, position))
}

fn command_path(typed: &[String]) -> Vec<String> {
    catalog()
        .iter()
        .filter(|entry| {
            entry.path.len() <= typed.len()
                && entry
                    .path
                    .iter()
                    .enumerate()
                    .all(|(index, word)| typed[index] == *word)
        })
        .map(|entry| entry.path.clone())
        .max_by_key(Vec::len)
        .unwrap_or_default()
}

fn value_hint(typed: &[String], path: &[String], position: usize) -> Option<String> {
    if let Some(meta) = catalog::match_meta(typed) {
        let hint = meta.hint.trim();
        return (!hint.is_empty() && position == 0).then(|| hint.to_string());
    }
    catalog::positional_hint(path, position).map(describe)
}

fn describe((label, help): (String, String)) -> String {
    if help.is_empty() {
        label
    } else {
        format!("{label}  {help}")
    }
}

fn pending_flag(typed: &[String]) -> Option<String> {
    let last = typed.last()?;
    let name = last.strip_prefix("--")?;
    if name.is_empty() || name.contains('=') {
        return None;
    }
    let path = command_path(typed);
    catalog::flag_takes_value(&path, name).then(|| name.to_string())
}

fn flag_names(typed: &[String]) -> Vec<String> {
    let mut names = catalog::flag_names(&command_path(typed));
    names.push("--json".to_string());
    names
}

fn matching(mut names: Vec<String>, prefix: &str) -> Vec<String> {
    names.retain(|name| name.starts_with(prefix));
    names.sort();
    names.dedup();
    names
}

fn first_words() -> Vec<String> {
    let mut words: Vec<String> = catalog()
        .iter()
        .filter_map(|entry| entry.path.first().cloned())
        .collect();
    words.sort();
    words.dedup();
    words
}

pub fn command_names() -> Vec<String> {
    ReplCommand::command()
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates_clap_commands() {
        let parsed = parse_line(":runs list --status running --json").unwrap();
        let ParsedLine::Command {
            path, flags, json, ..
        } = parsed
        else {
            panic!("expected command");
        };
        assert_eq!(path, ["runs", "list"]);
        assert_eq!(flags["status"], ["running"]);
        assert!(json);
    }

    #[test]
    fn completion_is_derived_from_clap() {
        assert_eq!(complete(":workfl").options, ["workflows"]);
        assert_eq!(
            complete(":settings list --kind ").options,
            ["config", "secret"]
        );
    }
}
