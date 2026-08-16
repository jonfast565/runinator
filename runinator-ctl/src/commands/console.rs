//! durable WDL console for the terminal.

use super::*;

use std::io::{IsTerminal, Read};

use reedline::{
    ColumnarMenu, DefaultPrompt, Emacs, KeyCode, KeyModifiers, MenuBuilder, Reedline,
    ReedlineEvent, ReedlineMenu, Signal, ValidationResult, Validator, default_emacs_keybindings,
};
use runinator_models::console::{ConsoleCell, ConsoleCellStatus, ConsoleSession, NewConsoleCell};

use crate::tui;

use super::repl;
use super::repl_completer::ReplCompleter;

struct WdlValidator;

impl Validator for WdlValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        let mut stack = Vec::new();
        let mut quote = None;
        let mut escaped = false;
        for character in line.chars() {
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
        if quote.is_some() || !stack.is_empty() || line.trim_end().ends_with('\\') {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn console(
    client: &Client,
    requested_session: Option<&str>,
    new_session: Option<&str>,
    execute: Option<&str>,
    file: Option<&Path>,
    no_follow: bool,
    json_output: bool,
    api_base_url: &str,
    plain: bool,
) -> Result<()> {
    if execute.is_some() && file.is_some() {
        return Err(err("use --execute or --file, not both"));
    }
    if requested_session.is_some() && new_session.is_some() {
        return Err(err("use --session or --new, not both"));
    }

    let session = select_session(client, requested_session, new_session).await?;
    let source = match (execute, file) {
        (Some(source), None) => Some(source.to_string()),
        (None, Some(path)) => Some(fs::read_to_string(path)?),
        (None, None) if !std::io::stdin().is_terminal() => {
            let mut source = String::new();
            std::io::stdin().read_to_string(&mut source)?;
            Some(source)
        }
        _ => None,
    };
    if let Some(source) = source {
        let cell = submit(client, &session, &source, no_follow).await?;
        return if json_output {
            output::json(&cell)
        } else {
            print_cell(&cell)
        };
    }

    println!("session {} ({})", session.name, session.id);
    println!(
        "a bare line is WDL; a `:` line is a runinatorctl command. :help lists both, Tab completes,"
    );
    println!("Ctrl+D exits and Ctrl+C clears the prompt.");

    // the terminal ui needs a real terminal: it draws an inline viewport, which it can only place by
    // asking the terminal where the cursor is. a pipe cannot answer, so that case takes the plain
    // prompt rather than failing.
    if !plain && std::io::stdout().is_terminal() {
        match tui::Prompt::new(session.name.clone(), api_base_url.to_string()) {
            Ok(prompt) => {
                return tui_console(
                    client,
                    prompt.with_history(read_history()),
                    session,
                    no_follow,
                    json_output,
                    api_base_url,
                )
                .await;
            }
            Err(error) => eprintln!("terminal ui unavailable ({error}); using the plain prompt"),
        }
    }

    plain_console(client, session, no_follow, json_output, api_base_url).await
}

// the plain prompt: one reedline line at a time, for a terminal that cannot host the ui or an
// operator who asked for it with `--plain`.
async fn plain_console(
    client: &Client,
    mut session: ConsoleSession,
    no_follow: bool,
    json_output: bool,
    api_base_url: &str,
) -> Result<()> {
    let mut editor = line_editor();
    let prompt = DefaultPrompt::default();
    let mut current_cell = None;
    loop {
        match editor.read_line(&prompt) {
            Ok(Signal::Success(source)) => {
                let source = source.trim();
                if source.is_empty() {
                    continue;
                }
                if source.starts_with(':') {
                    // a failing command must not end the session: the repl reports it and returns
                    // to the prompt, the way a shell does.
                    match handle_command(
                        client,
                        &mut session,
                        source,
                        current_cell,
                        no_follow,
                        api_base_url,
                    )
                    .await
                    {
                        Ok(CommandOutcome::Continue(cell)) => {
                            current_cell = cell.or(current_cell);
                        }
                        Ok(CommandOutcome::Exit) => break,
                        Err(error) => eprintln!("error: {error}"),
                    }
                    continue;
                }
                match submit(client, &session, source, no_follow).await {
                    Ok(cell) => {
                        current_cell = Some(cell.id);
                        if json_output {
                            output::json(&cell)?;
                        } else {
                            print_cell(&cell)?;
                        }
                    }
                    Err(error) => eprintln!("error: {error}"),
                }
            }
            Ok(Signal::CtrlC) => continue,
            Ok(Signal::CtrlD) => break,
            Ok(_) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

// the terminal-ui console: the same loop as the plain one, with the prompt drawn in an inline
// viewport that is suspended while a command prints.
async fn tui_console(
    client: &Client,
    mut prompt: tui::Prompt,
    mut session: ConsoleSession,
    no_follow: bool,
    json_output: bool,
    api_base_url: &str,
) -> Result<()> {
    let mut current_cell = None;

    loop {
        let source = match prompt.read_line()? {
            tui::Submission::Exit => break,
            tui::Submission::Line(source) => source,
        };
        let trimmed = source.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        // the ui gets out of the way for the duration of the command: its output is ordinary
        // stdout, and Ctrl+C has to reach it as a signal.
        prompt.suspend()?;
        prompt.echo(&trimmed);
        let note = if trimmed.starts_with(':') {
            match handle_command(
                client,
                &mut session,
                &trimmed,
                current_cell,
                no_follow,
                api_base_url,
            )
            .await
            {
                Ok(CommandOutcome::Continue(cell)) => {
                    current_cell = cell.or(current_cell);
                    None
                }
                Ok(CommandOutcome::Exit) => break,
                Err(error) => Some(error.to_string()),
            }
        } else {
            match submit(client, &session, &trimmed, no_follow).await {
                Ok(cell) => {
                    current_cell = Some(cell.id);
                    let printed = if json_output {
                        output::json(&cell)
                    } else {
                        print_cell(&cell)
                    };
                    printed.err().map(|error| error.to_string())
                }
                Err(error) => Some(error.to_string()),
            }
        };

        prompt.set_session(session.name.clone());
        prompt.set_note(note);
        prompt.resume()?;
    }

    write_history(&prompt.history());
    Ok(())
}

// history is a convenience: a home directory that cannot be read or written leaves the console
// usable rather than refusing to open it.
fn read_history() -> Vec<String> {
    let Ok(path) = history_file() else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.replace("\\n", "\n"))
        .collect()
}

fn write_history(history: &[String]) {
    let Ok(path) = history_file() else {
        return;
    };
    // one line per entry, so a multi-line cell is escaped rather than splitting into several.
    let body = history
        .iter()
        .rev()
        .take(HISTORY_ENTRIES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| line.replace('\n', "\\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = fs::write(path, body);
}

// the line editor: multiline WDL, tab completion over the command surface, and history that
// outlives the session so a command typed yesterday is still one arrow-up away.
fn line_editor() -> Reedline {
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );

    let editor = Reedline::create()
        .with_validator(Box::new(WdlValidator))
        .with_completer(Box::new(ReplCompleter))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(
            ColumnarMenu::default().with_name("completion_menu"),
        )))
        .with_edit_mode(Box::new(Emacs::new(keybindings)));

    // history is a convenience, so a home directory that cannot be written keeps the repl usable
    // rather than refusing to open it.
    match history_file().and_then(|path| {
        Ok(reedline::FileBackedHistory::with_file(
            HISTORY_ENTRIES,
            path,
        )?)
    }) {
        Ok(history) => editor.with_history(Box::new(history)),
        Err(_) => editor,
    }
}

fn history_file() -> Result<PathBuf> {
    fs::create_dir_all(runinator_utilities::app_data::app_data_dir()?)?;
    runinator_utilities::app_data::app_data_path(HISTORY_FILE)
}

const HISTORY_FILE: &str = "ctl-console-history.txt";
const HISTORY_ENTRIES: usize = 2_000;

async fn select_session(
    client: &Client,
    requested: Option<&str>,
    new_session: Option<&str>,
) -> Result<ConsoleSession> {
    if let Some(name) = new_session {
        return Ok(client.create_console_session(name).await?);
    }
    let sessions = client.console_sessions().await?;
    if let Some(requested) = requested {
        let id = requested.parse::<Uuid>().ok();
        return sessions
            .into_iter()
            .find(|session| Some(session.id) == id || session.name == requested)
            .ok_or_else(|| err(format!("console session '{requested}' not found")));
    }
    match sessions.into_iter().next() {
        Some(session) => Ok(session),
        None => Ok(client.create_console_session("scratch").await?),
    }
}

async fn submit(
    client: &Client,
    session: &ConsoleSession,
    source: &str,
    no_follow: bool,
) -> Result<ConsoleCell> {
    let cell = client
        .create_console_cell(
            session.id,
            &NewConsoleCell {
                source: source.to_string(),
                label: None,
                position: None,
            },
        )
        .await?;
    let cell = client.run_console_cell(cell.id).await?;
    if no_follow || cell.status != ConsoleCellStatus::Running {
        return Ok(cell);
    }
    eprintln!(
        "running cell {}{}",
        cell.id,
        cell.workflow_run_id
            .map(|id| format!(" (workflow run {id})"))
            .unwrap_or_default()
    );
    follow_cell(client, cell).await
}

enum CommandOutcome {
    Continue(Option<Uuid>),
    Exit,
}

async fn handle_command(
    client: &Client,
    session: &mut ConsoleSession,
    command: &str,
    current_cell: Option<Uuid>,
    no_follow: bool,
    api_base_url: &str,
) -> Result<CommandOutcome> {
    let mut parts = command.splitn(2, char::is_whitespace);
    let verb = parts.next().unwrap_or_default();
    let argument = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match verb {
        ":help" => print!("{}", repl::help(argument)?),
        ":clear" => {
            // the ansi erase-display + cursor-home pair every terminal this repl runs in supports.
            print!("\x1b[2J\x1b[H");
            io::Write::flush(&mut io::stdout())?;
        }
        ":sessions" => print_sessions(&client.console_sessions().await?),
        ":new" => {
            let name = argument.ok_or_else(|| err(":new requires a name"))?;
            *session = client.create_console_session(name).await?;
            println!("using session {} ({})", session.name, session.id);
        }
        ":use" => {
            let requested = argument.ok_or_else(|| err(":use requires a name or uuid"))?;
            *session = select_session(client, Some(requested), None).await?;
            println!("using session {} ({})", session.name, session.id);
        }
        ":history" => {
            let detail = client.console_session(session.id).await?;
            for cell in detail.cells {
                println!(
                    "{:>4} {:<10} {}  {}",
                    cell.position,
                    cell.status.as_str(),
                    cell.id,
                    one_line(&cell.source)
                );
            }
        }
        ":bindings" => {
            let detail = client.console_session(session.id).await?;
            for binding in detail.bindings {
                println!(
                    "{} = {}",
                    binding.name,
                    serde_json::to_string_pretty(&binding.value)?
                );
            }
        }
        ":cancel" => {
            let cell_id = argument
                .map(str::parse)
                .transpose()
                .map_err(|_| err("invalid cell uuid"))?
                .or(current_cell)
                .ok_or_else(|| err(":cancel requires a cell uuid"))?;
            let response = client.cancel_console_cell(cell_id).await?;
            println!("{}", response.message);
            return Ok(CommandOutcome::Continue(Some(cell_id)));
        }
        ":replay" => {
            let cell_id = argument
                .map(str::parse)
                .transpose()
                .map_err(|_| err("invalid cell uuid"))?
                .or(current_cell)
                .ok_or_else(|| err(":replay requires a cell uuid"))?;
            let mut cell = client.replay_console_cell(cell_id).await?;
            if !no_follow && cell.status == ConsoleCellStatus::Running {
                cell = follow_cell(client, cell).await?;
            }
            print_cell(&cell)?;
            return Ok(CommandOutcome::Continue(Some(cell_id)));
        }
        ":run" => {
            let argument = argument.ok_or_else(|| err(":run requires workflow or pipeline"))?;
            let (kind, rest) = argument
                .split_once(char::is_whitespace)
                .ok_or_else(|| err(":run requires a target"))?;
            let (target, parameters) = target_and_json(rest)?;
            match kind {
                "workflow" => {
                    let workflow = fetch_workflow_ref(client, &target).await?;
                    let workflow_id = workflow.id.ok_or_else(|| err("workflow has no id"))?;
                    let run = client.create_workflow_run(workflow_id, parameters).await?;
                    println!("workflow run {}", run.id);
                    if !no_follow {
                        follow_workflow(client, run.id).await?;
                    }
                }
                "pipeline" => {
                    let id = match target.parse::<Uuid>() {
                        Ok(id) => id,
                        Err(_) => client
                            .fetch_pipelines()
                            .await?
                            .into_iter()
                            .find(|pipeline| pipeline.name == target)
                            .and_then(|pipeline| pipeline.id)
                            .ok_or_else(|| err(format!("pipeline '{target}' not found")))?,
                    };
                    let run = client.create_pipeline_run(id, parameters).await?;
                    println!("pipeline run {}", run.id);
                    if !no_follow {
                        follow_pipeline(client, run.id).await?;
                    }
                }
                _ => return Err(err(":run supports workflow or pipeline")),
            }
        }
        ":invoke" => {
            let argument = argument.ok_or_else(|| err(":invoke requires package.export"))?;
            let (head, input) = target_and_json(argument)?;
            let mut words = head.split_whitespace();
            let target = words.next().ok_or_else(|| err("missing function target"))?;
            let (package, export) = target
                .rsplit_once('.')
                .ok_or_else(|| err("function target must be package.export"))?;
            let mut alias = None;
            let mut version = None;
            match (words.next(), words.next()) {
                (Some("alias"), Some(value)) => alias = Some(value),
                (Some("version"), Some(value)) => version = Some(value.parse::<i64>()?),
                (None, None) => {}
                _ => return Err(err("use alias <name> or version <number>")),
            }
            let result = client
                .invoke_function(package, export, alias, version, &input)
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        ":exit" | ":quit" => return Ok(CommandOutcome::Exit),
        // anything else is a command-line command: same parser, same flags, same help as the
        // process reached by `runinatorctl <verb>`.
        other => return command_line(client, other, argument, api_base_url).await,
    }
    Ok(CommandOutcome::Continue(None))
}

// run one `runinatorctl` command from inside the repl.
async fn command_line(
    client: &Client,
    verb: &str,
    argument: Option<&str>,
    api_base_url: &str,
) -> Result<CommandOutcome> {
    let mut tokens = vec![verb.trim_start_matches(':').to_string()];
    tokens.extend(repl::tokenize(argument.unwrap_or_default())?);
    let parsed = repl::parse(&tokens)?;
    // `:console` would open a second console inside this one; the session verbs already move
    // between sessions, so point at them instead of nesting.
    if matches!(parsed.command, Commands::Console { .. }) {
        return Err(err("already in a console; use :use, :new, or :sessions"));
    }
    // login rebuilds the client this repl is holding, so it belongs to the process rather than to a
    // session that would keep using the old credentials.
    if matches!(parsed.command, Commands::Login | Commands::Logout) {
        return Err(err(
            "run `runinatorctl login` or `logout` outside the console; this session is already authenticated",
        ));
    }

    // a long command (a watch, a dev loop) stays interruptible: Ctrl+C ends it and returns to the
    // prompt instead of killing the repl.
    //
    // the dispatcher can reach this console again (the `:console` guard above is what stops it), so
    // the future is boxed to keep it a finite size.
    let dispatch = Box::pin(super::run_command(
        client,
        &parsed.command,
        api_base_url,
        parsed.json,
    ));
    tokio::select! {
        result = dispatch => result?,
        interrupted = tokio::signal::ctrl_c() => {
            interrupted?;
            eprintln!("interrupted");
        }
    }
    Ok(CommandOutcome::Continue(None))
}

async fn follow_cell(client: &Client, mut cell: ConsoleCell) -> Result<ConsoleCell> {
    loop {
        tokio::select! {
            interrupted = tokio::signal::ctrl_c() => {
                interrupted?;
                eprintln!("detached; remote work is still running (use :cancel {})", cell.id);
                return Ok(cell);
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                cell = client.console_cell(cell.id).await?;
                if cell.status != ConsoleCellStatus::Running {
                    return Ok(cell);
                }
            }
        }
    }
}

fn target_and_json(value: &str) -> Result<(String, Value)> {
    let (target, parameters) = match value.split_once(" with ") {
        Some((target, json)) => (target, serde_json::from_str::<Value>(json)?),
        None => (value, Value::Object(Default::default())),
    };
    let target = target.trim().trim_matches('"').to_string();
    if target.is_empty() {
        return Err(err("target is required"));
    }
    Ok((target, parameters))
}

async fn follow_workflow(client: &Client, run_id: Uuid) -> Result<()> {
    loop {
        tokio::select! {
            interrupted = tokio::signal::ctrl_c() => {
                interrupted?;
                eprintln!("detached from workflow run {run_id}");
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                let (run, nodes) = client.fetch_workflow_run(run_id).await?;
                if run.status.is_terminal() {
                    println!("[{}] workflow run {run_id}", run.status.as_str());
                    if let Some(result) = nodes.iter().filter_map(|node| node.output_json.as_ref()).next_back() {
                        println!("{}", serde_json::to_string_pretty(result)?);
                    }
                    return Ok(());
                }
            }
        }
    }
}

async fn follow_pipeline(client: &Client, run_id: Uuid) -> Result<()> {
    loop {
        tokio::select! {
            interrupted = tokio::signal::ctrl_c() => {
                interrupted?;
                eprintln!("detached from pipeline run {run_id}");
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                let detail = client.fetch_pipeline_run(run_id).await?;
                if detail.run.status.is_terminal() {
                    println!("[{}] pipeline run {run_id}", detail.run.status.as_str());
                    return Ok(());
                }
            }
        }
    }
}

fn print_sessions(sessions: &[ConsoleSession]) {
    for session in sessions {
        println!(
            "{}  {:<24} {}",
            session.id,
            session.name,
            session.updated_at.to_rfc3339()
        );
    }
}

fn print_cell(cell: &ConsoleCell) -> Result<()> {
    println!("[{}] cell {}", cell.status.as_str(), cell.id);
    if let Some(result) = &cell.result {
        println!("{}", serde_json::to_string_pretty(result)?);
    }
    if let Some(error) = &cell.error {
        eprintln!("{error}");
    }
    if cell.status == ConsoleCellStatus::Running
        && let Some(run_id) = cell.workflow_run_id
    {
        println!("workflow run: {run_id}");
    }
    Ok(())
}

fn one_line(source: &str) -> String {
    output::truncate(&source.split_whitespace().collect::<Vec<_>>().join(" "), 72)
}

#[cfg(test)]
#[path = "console_tests.rs"]
mod tests;
