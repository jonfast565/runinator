use std::{thread, time::Duration};

use chrono::{DateTime, Local};

use crate::{
    config::Paths,
    snapshot::{StateSnapshot, read_snapshot},
    types::DynError,
};

pub fn show_status(paths: &Paths, watch: bool) -> Result<(), DynError> {
    loop {
        match read_snapshot(&paths.state_file) {
            Ok(snapshot) => {
                if watch {
                    clear_screen();
                }
                render_snapshot(&snapshot);
            }
            Err(err) => {
                if watch {
                    clear_screen();
                }
                println!("No supervisor state available: {}", err);
                if !watch {
                    return Ok(());
                }
            }
        }

        if !watch {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }
}

pub fn render_snapshot(snapshot: &StateSnapshot) {
    println!("Runinator Supervisor");
    println!("PID: {}", snapshot.supervisor_pid);
    println!("Config: {}", snapshot.config_path);
    println!(
        "Started: {}",
        human_time(&snapshot.started_at).unwrap_or_else(|| snapshot.started_at.clone())
    );
    println!(
        "Updated: {}",
        human_time(&snapshot.updated_at).unwrap_or_else(|| snapshot.updated_at.clone())
    );
    println!();

    let headers = [
        "process",
        "status",
        "pid",
        "restarts",
        "uptime",
        "exit",
        "command",
    ];

    let mut rows = Vec::with_capacity(snapshot.processes.len());
    for process in &snapshot.processes {
        rows.push(vec![
            process.name.clone(),
            process.status.clone(),
            process
                .pid
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            process.restarts.to_string(),
            process
                .uptime_seconds
                .map(format_uptime)
                .unwrap_or_else(|| "-".to_string()),
            process
                .last_exit_code
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            truncate_cell(&process.command, 52),
        ]);
    }

    print_table(&headers, &rows);
}

pub fn clear_screen() {
    print!("\x1B[2J\x1B[H");
}

fn truncate_cell(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut out = String::new();
    for ch in value.chars().take(max_chars.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = headers.iter().map(|v| v.len()).collect();
    for row in rows {
        for (idx, value) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(value.chars().count());
        }
    }

    print_border('╔', '╦', '╗', &widths);
    print_row(
        &headers.iter().map(|v| (*v).to_string()).collect::<Vec<_>>(),
        &widths,
    );
    print_border('╠', '╬', '╣', &widths);
    for row in rows {
        print_row(row, &widths);
    }
    print_border('╚', '╩', '╝', &widths);
}

fn print_border(left: char, middle: char, right: char, widths: &[usize]) {
    print!("{}", left);
    for (idx, width) in widths.iter().enumerate() {
        print!("{}", "═".repeat(*width + 2));
        if idx + 1 == widths.len() {
            print!("{}", right);
        } else {
            print!("{}", middle);
        }
    }
    println!();
}

fn print_row(values: &[String], widths: &[usize]) {
    print!("║");
    for (idx, value) in values.iter().enumerate() {
        let width = widths[idx];
        let padding = width.saturating_sub(value.chars().count());
        print!(" {}{} ║", value, " ".repeat(padding));
    }
    println!();
}

fn format_uptime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours:02}:{minutes:02}:{secs:02}")
}

fn human_time(input: &str) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(input).ok()?;
    let local = parsed.with_timezone(&Local);
    Some(local.format("%Y-%m-%d %H:%M:%S %Z").to_string())
}
