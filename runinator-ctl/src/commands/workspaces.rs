//! Durable workspace browsing and archive jobs.
use super::*;
use runinator_ctl_core::cli::WorkspaceCommands;

pub(super) async fn run(client: &Client, command: &WorkspaceCommands) -> Result<()> {
    match command {
        WorkspaceCommands::List { offset } => {
            output::json(&client.list_durable_workspaces(*offset).await?)
        }
        WorkspaceCommands::Create { key } => {
            output::json(&client.create_durable_workspace(key).await?)
        }
        WorkspaceCommands::Versions { workspace, offset } => {
            output::json(&client.workspace_versions(*workspace, *offset).await?)
        }
        WorkspaceCommands::Ls {
            workspace,
            version,
            path,
            cursor,
            results,
        } => {
            let page = if *results {
                client
                    .workspace_results(*workspace, *version, cursor.as_deref())
                    .await?
            } else {
                client
                    .workspace_directory(*workspace, *version, path, cursor.as_deref())
                    .await?
            };
            output::json(&page)
        }
        WorkspaceCommands::Cat {
            workspace,
            version,
            path,
            result,
        } => {
            use std::io::Write;
            let bytes = client
                .workspace_preview(*workspace, *version, path, *result)
                .await?;
            std::io::stdout().write_all(&bytes)?;
            Ok(())
        }
        WorkspaceCommands::Diff {
            workspace,
            before,
            after,
            cursor,
        } => output::json(
            &client
                .workspace_diff(*workspace, *before, *after, cursor.as_deref())
                .await?,
        ),
        WorkspaceCommands::Import { key, archive } => {
            let file = tokio::fs::File::open(archive).await?;
            let workspace = client.create_durable_workspace(key).await?;
            let job = client
                .create_workspace_transfer(workspace.id, 0, true, false)
                .await?;
            eprintln!("Uploading transfer {}", job.id);
            output::json(&client.upload_workspace_transfer(job.id, file).await?)
        }
        WorkspaceCommands::Export {
            workspace,
            version,
            filesystem,
        } => output::json(
            &client
                .create_workspace_transfer(*workspace, *version, false, *filesystem)
                .await?,
        ),
        WorkspaceCommands::Job { id } => output::json(&client.workspace_transfer(*id).await?),
        WorkspaceCommands::Cancel { id } => {
            client.cancel_workspace_transfer(*id).await?;
            Ok(())
        }
        WorkspaceCommands::Download { id, destination } => {
            use tokio::io::AsyncWriteExt;
            let temporary = destination.with_extension(format!("{}.partial", Uuid::new_v4()));
            let result = async {
                let mut response = client.workspace_transfer_stream(*id).await?;
                let mut file = tokio::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&temporary)
                    .await?;
                while let Some(chunk) = response.chunk().await? {
                    file.write_all(&chunk).await?;
                }
                file.sync_all().await?;
                drop(file);
                // a hard link publishes atomically without overwriting a pre-existing destination.
                tokio::fs::hard_link(&temporary, destination).await?;
                Ok::<_, Box<dyn Error + Send + Sync>>(())
            }
            .await;
            let _ = tokio::fs::remove_file(temporary).await;
            result
        }
    }
}
