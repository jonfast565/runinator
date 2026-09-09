use std::io::{self, IsTerminal, Write};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::commands::Result;

pub fn json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Renders a serializable result as a terminal table.
///
/// Read commands use this for their human-facing view while `--json` retains the complete response
/// structure. Objects become field/value rows; homogeneous object lists become one row per item.
pub fn value_table<T: Serialize>(value: &T) -> Result<String> {
    Ok(json_value_table(&serde_json::to_value(value)?))
}

pub fn json_value_table(value: &Value) -> String {
    match value {
        Value::Object(fields) => table(
            &["field", "value"],
            &fields
                .iter()
                .map(|(field, value)| vec![field.clone(), json_cell(value)])
                .collect::<Vec<_>>(),
        ),
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_object) => {
            object_list_table(items)
        }
        Value::Array(items) => table(
            &["value"],
            &items
                .iter()
                .map(|item| vec![json_cell(item)])
                .collect::<Vec<_>>(),
        ),
        value => table(&["value"], &[vec![json_cell(value)]]),
    }
}

fn object_list_table(items: &[Value]) -> String {
    let mut columns = Vec::new();
    for item in items {
        let Some(fields) = item.as_object() else {
            continue;
        };
        for field in fields.keys() {
            if !columns.contains(field) {
                columns.push(field.clone());
            }
        }
    }
    let rows = items
        .iter()
        .filter_map(Value::as_object)
        .map(|fields| {
            columns
                .iter()
                .map(|column| {
                    fields
                        .get(column)
                        .map(json_cell)
                        .unwrap_or_else(|| "-".into())
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    table(
        &columns.iter().map(String::as_str).collect::<Vec<_>>(),
        &rows,
    )
}

fn json_cell(value: &Value) -> String {
    match value {
        Value::Null => "-".into(),
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

pub fn time(value: Option<DateTime<Utc>>) -> String {
    value
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| "-".into())
}

/// a header row and padded columns, the shape the web console prints every listing in.
///
/// the last column is never padded and never truncated by width alone: it holds the summary or the
/// message, and a trailing run of spaces would only make a copied line messier.
pub fn table(columns: &[&str], rows: &[Vec<String>]) -> String {
    let widths = column_widths(columns, rows);
    let mut text = String::new();
    text.push_str(&join_row(
        &columns
            .iter()
            .map(|column| column.to_string())
            .collect::<Vec<_>>(),
        &widths,
    ));
    for row in rows {
        text.push_str(&join_row(row, &widths));
    }
    text
}

// every column is as wide as its widest cell, so the terminal table lines up the way the web
// console's grid does.
fn column_widths(columns: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = columns
        .iter()
        .map(|column| column.chars().count())
        .collect();
    for row in rows {
        for (index, value) in row.iter().enumerate() {
            let width = value.chars().count();
            match widths.get_mut(index) {
                Some(current) => *current = (*current).max(width),
                None => widths.push(width),
            }
        }
    }
    widths
}

fn join_row(row: &[String], widths: &[usize]) -> String {
    let last = row.len().saturating_sub(1);
    let mut line = String::new();
    for (index, value) in row.iter().enumerate() {
        if index == last {
            line.push_str(value.trim_end());
            break;
        }
        let pad = widths
            .get(index)
            .copied()
            .unwrap_or(0)
            .saturating_sub(value.chars().count());
        line.push_str(value);
        line.push_str(&" ".repeat(pad + 2));
    }
    line.push('\n');
    line
}

/// how wide the terminal is, for deciding what to truncate. eighty when it cannot be asked.
pub fn terminal_width() -> usize {
    ratatui::crossterm::terminal::size()
        .map(|(columns, _)| columns as usize)
        .unwrap_or(80)
        .max(40)
}

pub fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.into();
    }

    let mut out = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod output_tests;

/// Controls repeated command output. Human-facing watches redraw in place on a terminal and emit
/// separated snapshots when redirected; JSON is always an append-only stream of documents.
pub struct LiveDisplay {
    first: bool,
    in_place: bool,
    machine_output: bool,
}

impl LiveDisplay {
    pub fn new(machine_output: bool) -> Self {
        Self {
            first: true,
            in_place: !machine_output && io::stdout().is_terminal(),
            machine_output,
        }
    }

    pub fn begin_frame(&mut self) {
        if !self.first {
            if self.in_place {
                // Clear the previous frame and return to the top-left. Keeping the first frame in
                // normal scrollback avoids erasing the command that launched the watch.
                print!("\x1b[2J\x1b[H");
            } else if !self.machine_output {
                println!();
            }
        }
        self.first = false;
    }

    pub fn flush(&self) -> io::Result<()> {
        io::stdout().flush()
    }
}
