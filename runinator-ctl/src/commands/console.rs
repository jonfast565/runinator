//! durable REXRAP console for the terminal.

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

struct RexRapValidator;

pub(super) struct ConsoleRequest<'a> {
    pub requested_session: Option<&'a str>,
    pub new_session: Option<&'a str>,
    pub execute: Option<&'a str>,
    pub file: Option<&'a Path>,
    pub no_follow: bool,
    pub json_output: bool,
    pub api_base_url: &'a str,
    pub plain: bool,
}

impl Validator for RexRapValidator {
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
pub(super) async fn console(client: &Client, request: ConsoleRequest<'_>) -> Result<()> {
    let ConsoleRequest {
        requested_session,
        new_session,
        execute,
        file,
        no_follow,
        json_output,
        api_base_url,
        plain,
    } = request;
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

    // The terminal UI needs a real terminal and control of stdout so its output pane can scroll.
    // A pipe provides neither, so use the plain prompt instead.
    if !plain && std::io::stdout().is_terminal() {
        match tui::Prompt::new(session.name.clone(), api_base_url.to_string()) {
            Ok(prompt) => {
                // Print after the UI starts, so the greeting is the first line in its output pane.
                greet(&session, true);
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

    greet(&session, false);
    plain_console(client, session, no_follow, json_output, api_base_url).await
}

// the banner and what the console can do, in the few lines an operator reads once.
//
// printed rather than drawn as a band of the frame: it goes down the same pipe a command's output
// does, so it starts at the top of the scrollable pane, scrolls away as work arrives, and is
// replayed to the shell on the way out — none of which a fixed header would do, and it costs no
// rows on a short terminal.
fn greet(session: &ConsoleSession, scrollable: bool) {
    // `main` skips the banner for this command so that this is the only one.
    println!("{}", crate::banner::text());
    println!();
    println!("session {} ({})", session.name, session.id);
    println!(
        "a bare line is REXRAP; a `:` line is a runinatorctl command. :help lists both, Tab completes,"
    );
    println!("Ctrl+D exits and Ctrl+C clears the prompt.");
    if scrollable {
        println!(
            "PgUp/PgDn or the wheel scrolls this output (Shift+End follows); \
             hold Shift to select text."
        );
    }
}

// The plain prompt reads one reedline line at a time. Use it when the terminal cannot host the UI or an
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

// The terminal-UI console uses the same loop as the plain prompt and draws the UI throughout. A command's
// output arrives in the scrollable pane rather than on the terminal, so nothing is suspended around
// it and a long run can be read while it is still going.
//
// Ctrl+C is answered by dropping the command's future, since raw mode means the keystroke never
// becomes a signal. dropping cancels whatever it was awaiting, which is why the cell id is recorded
// before the follow rather than after it — an interrupted follow still leaves `:cancel` a target.
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
        prompt.echo(&trimmed);

        let note = if trimmed.starts_with(':') {
            let command = handle_command(
                client,
                &mut session,
                &trimmed,
                current_cell,
                no_follow,
                api_base_url,
            );
            match prompt.run(command).await? {
                None => Some("interrupted".to_string()),
                Some(Ok(CommandOutcome::Continue(cell))) => {
                    current_cell = cell.or(current_cell);
                    None
                }
                Some(Ok(CommandOutcome::Exit)) => break,
                Some(Err(error)) => Some(error.to_string()),
            }
        } else {
            cell_line(
                client,
                &mut prompt,
                &session,
                &trimmed,
                &mut current_cell,
                no_follow,
                json_output,
            )
            .await?
        };

        prompt.set_session(session.name.clone());
        prompt.set_note(note);
    }

    write_history(&prompt.history());
    Ok(())
}

// Run one REXRAP cell from the terminal UI. Start it first, then
// wait for it. the note it returns is what the prompt shows under the input.
async fn cell_line(
    client: &Client,
    prompt: &mut tui::Prompt,
    session: &ConsoleSession,
    source: &str,
    current_cell: &mut Option<Uuid>,
    no_follow: bool,
    json_output: bool,
) -> Result<Option<String>> {
    let cell = match prompt.run(start_cell(client, session, source)).await? {
        None => return Ok(Some("interrupted".to_string())),
        Some(Err(error)) => return Ok(Some(error.to_string())),
        Some(Ok(cell)) => cell,
    };
    *current_cell = Some(cell.id);

    if no_follow || cell.status != ConsoleCellStatus::Running {
        return Ok(printed(&cell, json_output));
    }

    announce_cell(&cell);
    let id = cell.id;
    match prompt.run(follow_cell(client, cell)).await? {
        None => Ok(Some(format!(
            "interrupted; remote work is still running (use :cancel {id})"
        ))),
        Some(Err(error)) => Ok(Some(error.to_string())),
        Some(Ok(cell)) => Ok(printed(&cell, json_output)),
    }
}

fn printed(cell: &ConsoleCell, json_output: bool) -> Option<String> {
    let printed = if json_output {
        output::json(cell)
    } else {
        print_cell(cell)
    };
    printed.err().map(|error| error.to_string())
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

// the line editor: multiline REXRAP, tab completion over the command surface, and history that
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
        .with_validator(Box::new(RexRapValidator))
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
    fs::create_dir_all(runinator_platform::app_data::app_data_dir()?)?;
    runinator_platform::app_data::app_data_path(HISTORY_FILE)
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
    let cell = start_cell(client, session, source).await?;
    if no_follow || cell.status != ConsoleCellStatus::Running {
        return Ok(cell);
    }
    announce_cell(&cell);
    follow_cell(client, cell).await
}

// create a cell and start it, without waiting for it to settle.
//
// separate from the follow so a caller that can be interrupted knows the cell's id before it starts
// waiting; a cell nobody can name is one nobody can cancel.
async fn start_cell(
    client: &Client,
    session: &ConsoleSession,
    source: &str,
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
    Ok(client.run_console_cell(cell.id).await?)
}

fn announce_cell(cell: &ConsoleCell) {
    eprintln!(
        "running cell {}{}",
        cell.id,
        cell.workflow_run_id
            .map(|id| format!(" (workflow run {id})"))
            .unwrap_or_default()
    );
}

enum CommandOutcome {
    Continue(Option<Uuid>),
    Exit,
}

/// read one `:` line and run what it names.
///
/// the line is tokenized once, then matched against the console-local verbs by longest path before
/// anything else is considered — the same order the web console resolves a line in. only when no
/// console verb claims it does the line go to clap, and only when clap has no such verb either is
/// it an error, which is where the nearest-verb suggestion comes from.
async fn handle_command(
    client: &Client,
    session: &mut ConsoleSession,
    command: &str,
    current_cell: Option<Uuid>,
    no_follow: bool,
    api_base_url: &str,
) -> Result<CommandOutcome> {
    let mut tokens = repl::scan(command.trim().trim_start_matches(':'))?;
    let Some(first) = tokens.first().map(|token| token.text.clone()) else {
        return Ok(CommandOutcome::Continue(None));
    };
    // `:quit` has always been accepted beside `:exit`; the catalog lists one of the two.
    if first == "quit" {
        tokens[0].text = "exit".to_string();
    }
    // `:functions` is the session library, while the process's `functions` command still has
    // subcommands such as `functions publish`. The console-local bare verb must not shadow that
    // clap surface; command lines with an additional word keep their normal dispatch path.
    if first == "functions" && tokens.len() > 1 {
        let words: Vec<String> = tokens.into_iter().map(|token| token.text).collect();
        return command_line(client, &words, api_base_url).await;
    }
    if let Some(matched) = repl::match_meta(&tokens) {
        return meta_command(client, session, &matched, current_cell, no_follow).await;
    }
    let words: Vec<String> = tokens.into_iter().map(|token| token.text).collect();
    if repl::command_names().contains(&words[0]) {
        return command_line(client, &words, api_base_url).await;
    }
    Err(err(repl::unknown_command(&words[0])))
}

// the console's own verbs: the ones that read a session, a cell, or the notebook, and so have no
// command-line counterpart to defer to.
async fn meta_command(
    client: &Client,
    session: &mut ConsoleSession,
    matched: &repl::MetaMatch,
    current_cell: Option<Uuid>,
    no_follow: bool,
) -> Result<CommandOutcome> {
    let arguments = &matched.arguments;
    match matched.command.path {
        ["help"] => {
            let topic = arguments.args.join(" ");
            print!(
                "{}",
                repl::help(Some(topic.as_str()).filter(|topic| !topic.is_empty()))?
            );
        }
        ["clear"] => {
            // the ansi erase-display + cursor-home pair every terminal this repl runs in supports.
            print!("\x1b[2J\x1b[H");
            io::Write::flush(&mut io::stdout())?;
        }
        ["sessions"] => print_sessions(&client.console_sessions().await?, session.id),
        ["new"] => {
            *session = client
                .create_console_session(arguments.required(0, "session name")?)
                .await?;
            println!("using session {} ({})", session.name, session.id);
        }
        ["use"] => {
            let requested = arguments.required(0, "session name or id")?;
            *session = select_session(client, Some(requested), None).await?;
            println!("using session {} ({})", session.name, session.id);
        }
        ["history"] => {
            let detail = client.console_session(session.id).await?;
            let rows = detail
                .cells
                .iter()
                .map(|cell| {
                    vec![
                        cell.position.to_string(),
                        cell.status.as_str().to_string(),
                        cell.id.to_string(),
                        one_line(&cell.source),
                    ]
                })
                .collect::<Vec<_>>();
            print!("{}", output::table(&["#", "status", "id", "source"], &rows));
        }
        ["bindings"] => {
            let detail = client.console_session(session.id).await?;
            let rows = detail
                .bindings
                .iter()
                .map(|binding| {
                    Ok(vec![
                        binding.name.clone(),
                        output::truncate(&serde_json::to_string(&binding.value)?, 72),
                    ])
                })
                .collect::<Result<Vec<_>>>()?;
            print!("{}", output::table(&["name", "value"], &rows));
        }
        ["functions"] => {
            let detail = client.console_session(session.id).await?;
            let rows = detail
                .functions
                .iter()
                .map(|function| {
                    vec![
                        function.name.clone(),
                        if function.is_task { "task fn" } else { "fn" }.to_string(),
                        output::truncate(&function.cell_id.to_string(), 14),
                        one_line(&function.source),
                    ]
                })
                .collect::<Vec<_>>();
            print!(
                "{}",
                output::table(&["name", "kind", "cell", "source"], &rows)
            );
        }
        ["cancel"] => {
            let cell_id = cell_reference(arguments, current_cell, "cancel")?;
            let response = client.cancel_console_cell(cell_id).await?;
            println!("{}", response.message);
            return Ok(CommandOutcome::Continue(Some(cell_id)));
        }
        ["replay"] => {
            let cell_id = cell_reference(arguments, current_cell, "replay")?;
            let mut cell = client.replay_console_cell(cell_id).await?;
            if !no_follow && cell.status == ConsoleCellStatus::Running {
                cell = follow_cell(client, cell).await?;
            }
            print_cell(&cell)?;
            return Ok(CommandOutcome::Continue(Some(cell_id)));
        }
        ["run", "workflow"] => {
            let target = arguments.required(0, "workflow")?.to_string();
            let parameters = run_parameters(arguments)?;
            let workflow = fetch_workflow_ref(client, &target).await?;
            let workflow_id = workflow.id.ok_or_else(|| err("workflow has no id"))?;
            let run = client
                .create_workflow_run_with_options(
                    workflow_id,
                    parameters,
                    arguments.is_set("debug"),
                    arguments.flag("name").map(str::to_string),
                )
                .await?;
            println!("workflow run {}", run.id);
            if !no_follow {
                follow_workflow(client, run.id).await?;
            }
        }
        ["run", "pipeline"] => {
            let target = arguments.required(0, "pipeline")?.to_string();
            let parameters = run_parameters(arguments)?;
            let pipeline = super::pipelines::resolve_pipeline(client, &target).await?;
            let id = pipeline
                .id
                .ok_or_else(|| err("pipeline has no persisted id"))?;
            let run = client.create_pipeline_run(id, parameters).await?;
            println!("pipeline run {}", run.id);
            if !no_follow {
                follow_pipeline(client, run.id).await?;
            }
        }
        ["invoke"] => {
            let target = arguments.required(0, "package.export")?;
            let (package, export) = target
                .rsplit_once('.')
                .ok_or_else(|| err("the function target must be package.export"))?;
            let (alias, version) = function_selector(arguments)?;
            let input = run_parameters(arguments)?;
            let result = client
                .invoke_function(package, export, alias.as_deref(), version, &input)
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        ["exit"] => return Ok(CommandOutcome::Exit),
        // every path in `META_COMMANDS` has an arm above; a new one without a body is a bug worth
        // reporting rather than silently doing nothing.
        other => return Err(err(format!("':{}' has no handler", other.join(" ")))),
    }
    Ok(CommandOutcome::Continue(None))
}

// `:cancel` and `:replay` act on a cell by UUID. Without one, they use the last cell in this session.
// started, which is what makes a bare `:cancel` mean "the thing i just ran".
fn cell_reference(
    arguments: &repl::Arguments,
    current_cell: Option<Uuid>,
    verb: &str,
) -> Result<Uuid> {
    arguments
        .arg(0)
        .map(str::parse)
        .transpose()
        .map_err(|_| err("invalid cell uuid"))?
        .or(current_cell)
        .ok_or_else(|| err(format!(":{verb} requires a cell uuid")))
}

// run parameters, from either spelling: `--param k=v` pairs, or a `… with {json}` tail. the web
// console accepts both too, so a line copied between the two consoles keeps working.
fn run_parameters(arguments: &repl::Arguments) -> Result<Value> {
    if let Some(json) = arguments.raw_after("with") {
        return Ok(serde_json::from_str(&json)?);
    }
    if let Some(input) = arguments.flag("input") {
        return Ok(serde_json::from_str(input)?);
    }
    params::load_object(None, arguments.flag_list("param"))
}

// which version of a packaged function to call: an alias resolves at call time, a version pins.
// the bare `alias production` / `version 3` spelling predates the flags and still works.
fn function_selector(arguments: &repl::Arguments) -> Result<(Option<String>, Option<i64>)> {
    let mut alias = arguments.flag("alias").map(str::to_string);
    let mut version = arguments
        .flag("version")
        .map(str::parse::<i64>)
        .transpose()?;
    match (arguments.arg(1), arguments.arg(2)) {
        (Some("alias"), Some(value)) => alias = Some(value.to_string()),
        (Some("version"), Some(value)) => version = Some(value.parse()?),
        _ => {}
    }
    Ok((alias, version))
}

// run one `runinatorctl` command from inside the repl.
async fn command_line(
    client: &Client,
    tokens: &[String],
    api_base_url: &str,
) -> Result<CommandOutcome> {
    let parsed = repl::parse(tokens)?;
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

async fn follow_workflow(client: &Client, run_id: Uuid) -> Result<()> {
    loop {
        tokio::select! {
            interrupted = tokio::signal::ctrl_c() => {
                interrupted?;
                eprintln!("detached from workflow run {run_id}");
                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                let run = client.fetch_workflow_run(run_id).await?;
                if run.status.is_terminal() {
                    println!("[{}] workflow run {run_id}", run.status.as_str());
                    // the run's last durable effect result is what a node output used to be.
                    let effects = client.fetch_workflow_effects(run_id).await.unwrap_or_default();
                    if let Some(result) = effects.iter().filter_map(|effect| effect.result.as_ref()).next_back() {
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

// the session in use is marked, the way the web console's `:sessions` marks it: a list of names
// with no "you are here" is a list you have to hold in your head.
fn print_sessions(sessions: &[ConsoleSession], active: Uuid) {
    let rows = sessions
        .iter()
        .map(|session| {
            vec![
                if session.id == active { "*" } else { " " }.to_string(),
                session.id.to_string(),
                output::truncate(&session.name, 28),
                session.updated_at.to_rfc3339(),
            ]
        })
        .collect::<Vec<_>>();
    print!("{}", output::table(&["", "id", "name", "updated"], &rows));
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
