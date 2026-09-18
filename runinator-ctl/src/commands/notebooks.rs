use super::*;

use runinator_ctl_core::cli::{NotebookCellCommands, NotebookSessionCommands};
use runinator_models::console::{ConsoleCell, ConsoleSession, NewConsoleCell};

pub(super) async fn notebooks(
    client: &Client,
    command: &NotebookCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        NotebookCommands::Sessions { command } => sessions(client, command, json_output).await,
        NotebookCommands::Cells { command } => cells(client, command, json_output).await,
    }
}

async fn sessions(
    client: &Client,
    command: &NotebookSessionCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        NotebookSessionCommands::List => {
            print_sessions(&client.console_sessions().await?, json_output)
        }
        NotebookSessionCommands::Show { id } => {
            let detail = client.console_session(*id).await?;
            if json_output {
                return output::json(&detail);
            }
            println!("{} ({})", detail.session.name, detail.session.id);
            print_cells(&detail.cells);
            Ok(())
        }
        NotebookSessionCommands::Create { name } => {
            let session = client.create_console_session(name).await?;
            print_session_result(&session, json_output)
        }
        NotebookSessionCommands::Rename { id, name } => {
            let detail = client.rename_console_session(*id, name).await?;
            print_session_result(&detail.session, json_output)
        }
        NotebookSessionCommands::Clear { id } => {
            let result = client.clear_console_session(*id).await?;
            print_value_result(result, json_output, format!("cleared notebook {id}"))
        }
        NotebookSessionCommands::Delete { id } => {
            let result = client.delete_console_session(*id).await?;
            print_value_result(result, json_output, format!("deleted notebook {id}"))
        }
    }
}

async fn cells(client: &Client, command: &NotebookCellCommands, json_output: bool) -> Result<()> {
    let cell = match command {
        NotebookCellCommands::Show { id } => client.console_cell(*id).await?,
        NotebookCellCommands::Create {
            session,
            file,
            label,
            position,
        } => {
            let request = cell_request(file, label.clone(), *position)?;
            client.create_console_cell(*session, &request).await?
        }
        NotebookCellCommands::Update {
            id,
            file,
            label,
            position,
        } => {
            let request = cell_request(file, label.clone(), *position)?;
            client.update_console_cell(*id, &request).await?
        }
        NotebookCellCommands::Run { id } => client.run_console_cell(*id).await?,
        NotebookCellCommands::Cancel { id } => {
            let result = client.cancel_console_cell(*id).await?;
            return print_task_response(result, "canceled console cell", json_output);
        }
        NotebookCellCommands::Replay { id } => client.replay_console_cell(*id).await?,
        NotebookCellCommands::Delete { id } => {
            let result = client.delete_console_cell(*id).await?;
            return print_value_result(result, json_output, format!("deleted console cell {id}"));
        }
    };
    print_cell_result(&cell, json_output)
}

fn cell_request(
    file: &Path,
    label: Option<String>,
    position: Option<i64>,
) -> Result<NewConsoleCell> {
    Ok(NewConsoleCell {
        source: fs::read_to_string(file)?,
        label,
        position,
    })
}

fn print_sessions(sessions: &[ConsoleSession], json_output: bool) -> Result<()> {
    if json_output {
        return output::json(&sessions);
    }
    let rows = sessions
        .iter()
        .map(|session| {
            vec![
                session.id.to_string(),
                session.name.clone(),
                session.updated_at.to_rfc3339(),
            ]
        })
        .collect::<Vec<_>>();
    print!("{}", output::table(&["ID", "NAME", "UPDATED"], &rows));
    Ok(())
}

fn print_session_result(session: &ConsoleSession, json_output: bool) -> Result<()> {
    if json_output {
        return output::json(session);
    }
    println!("{} ({})", session.name, session.id);
    Ok(())
}

fn print_cells(cells: &[ConsoleCell]) {
    let rows = cells
        .iter()
        .map(|cell| {
            vec![
                cell.id.to_string(),
                cell.position.to_string(),
                cell.label.clone().unwrap_or_else(|| "-".into()),
                format!("{:?}", cell.status).to_lowercase(),
            ]
        })
        .collect::<Vec<_>>();
    print!(
        "{}",
        output::table(&["ID", "POSITION", "LABEL", "STATUS"], &rows)
    );
}

fn print_cell_result(cell: &ConsoleCell, json_output: bool) -> Result<()> {
    if json_output {
        return output::json(cell);
    }
    print_cells(std::slice::from_ref(cell));
    Ok(())
}

fn print_value_result(value: Value, json_output: bool, message: String) -> Result<()> {
    if json_output {
        return output::json(&value);
    }
    println!("{message}");
    Ok(())
}
