use super::*;

pub(super) async fn files(
    client: &Client,
    command: &FileCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        FileCommands::List => {
            let files = client.list_workflow_files().await?;
            if json_output {
                return output::json(&files);
            }
            let rows = files
                .into_iter()
                .map(|file| {
                    vec![
                        file.descriptor.id.to_string(),
                        file.descriptor.path,
                        file.revision.to_string(),
                        file.descriptor.size_bytes.to_string(),
                        file.descriptor.mime_type,
                        file.archived.to_string(),
                    ]
                })
                .collect::<Vec<_>>();
            print!(
                "{}",
                output::table(
                    &["ID", "PATH", "REVISION", "BYTES", "MIME", "ARCHIVED"],
                    &rows,
                )
            );
        }
        FileCommands::Upload {
            source,
            path,
            mime_type,
        } => {
            let file = client
                .upload_workflow_file(path, mime_type.as_deref(), fs::read(source)?)
                .await?;
            if json_output {
                return output::json(&file);
            }
            println!(
                "uploaded {} revision {} ({})",
                file.descriptor.path, file.revision, file.descriptor.id
            );
        }
        FileCommands::Download { id, output: path } => {
            let bytes = client.download_workflow_file(*id).await?;
            fs::write(path, bytes)?;
            if json_output {
                return output::json(&json!({ "id": id, "output": path }));
            }
            println!("wrote {}", path.display());
        }
        FileCommands::Archive { id } => {
            let result = client.archive_workflow_file(*id).await?;
            if json_output {
                return output::json(&result);
            }
            println!("archived workflow file {id}");
        }
    }
    Ok(())
}
