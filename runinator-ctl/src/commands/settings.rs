use super::*;

pub(super) async fn settings(
    client: &Client,
    command: &SettingsCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        SettingsCommands::List { kind } => {
            let mut entries = client.list_settings().await?;
            if let Some(kind) = kind {
                let kind = SettingKind::from(*kind);
                entries.retain(|entry| entry.kind == kind);
            }
            if json_output {
                return output::json(&entries);
            }
            print_settings(&entries);
        }
        SettingsCommands::Get { scope, name, kind } => {
            let value = client
                .get_setting(SettingKind::from(*kind), scope, name)
                .await?;
            if json_output {
                return output::json(&value);
            }
            match &value {
                Value::String(text) => println!("{text}"),
                other => println!("{}", serde_json::to_string_pretty(other)?),
            }
        }
        SettingsCommands::Set {
            scope,
            name,
            value,
            value_file,
            kind,
            schema,
        } => {
            let kind = SettingKind::from(*kind);
            let raw = resolve_set_value(value.as_deref(), value_file.as_deref())?;
            // config values are json; secrets are passed through as a plain string.
            let value = match kind {
                SettingKind::Config => serde_json::from_str::<Value>(&raw)
                    .map_err(|e| err(format!("config value must be valid json: {e}")))?,
                SettingKind::Secret => Value::String(raw),
            };
            let schema = match schema {
                Some(text) => Some(
                    serde_json::from_str::<Value>(text)
                        .map_err(|e| err(format!("--schema must be valid json: {e}")))?,
                ),
                None => None,
            };
            let response = client
                .put_setting(kind, scope, name, &value, schema.as_ref())
                .await?;
            if json_output {
                return output::json(&response);
            }
            println!("stored {} {scope}/{name}", kind.as_str());
        }
        SettingsCommands::Import { file } => {
            if file.extension().and_then(|ext| ext.to_str()) != Some("rrx") {
                return Err(err(format!(
                    "settings import requires an .rrx file with a settings block, got {}",
                    file.display()
                )));
            }
            let data = fs::read_to_string(file)?;
            let blocks =
                runinator_rexrap::parse_rrx_blocks(&data).map_err(|e| err(e.render(&data)))?;
            let source = blocks.settings.join("\n");
            let bundle = runinator_rexrap::parse_secrets_str(&source).map_err(|e| {
                err(format!(
                    "failed to parse {}:\n{}",
                    file.display(),
                    e.render(&source)
                ))
            })?;
            let imported = client
                .import_pack(&WorkflowBundle::default(), Some(&bundle), None, false)
                .await?;
            if json_output {
                return output::json(&imported.secrets);
            }
            println!(
                "imported {} setting(s) from {}",
                imported.secrets.secrets.len(),
                file.display()
            );
        }
        SettingsCommands::Delete { scope, name, kind } => {
            let response = client
                .delete_setting(SettingKind::from(*kind), scope, name)
                .await?;
            if json_output {
                return output::json(&response);
            }
            println!(
                "deleted {} {scope}/{name}",
                SettingKind::from(*kind).as_str()
            );
        }
    }
    Ok(())
}

// resolves a set value from the inline argument or a file, requiring exactly one.
fn resolve_set_value(inline: Option<&str>, file: Option<&Path>) -> Result<String> {
    match (inline, file) {
        (Some(value), None) => Ok(value.to_string()),
        (None, Some(path)) => Ok(fs::read_to_string(path)?),
        (Some(_), Some(_)) => Err(err("provide either VALUE or --value-file, not both")),
        (None, None) => Err(err("a VALUE argument or --value-file is required")),
    }
}

fn print_settings(entries: &[runinator_models::settings::SettingSummary]) {
    println!("{:<8} {:<20} name", "kind", "scope");
    for entry in entries {
        println!(
            "{:<8} {:<20} {}",
            entry.kind.as_str(),
            output::truncate(&entry.scope, 20),
            entry.name
        );
    }
}
