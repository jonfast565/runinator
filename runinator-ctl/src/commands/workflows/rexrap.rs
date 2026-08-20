use super::*;

pub(crate) fn rexrap(command: &RexRapCommands, json_output: bool) -> Result<()> {
    match command {
        RexRapCommands::Compile {
            file,
            output,
            typing,
        } => {
            let source = fs::read_to_string(file)?;
            let options = runinator_rexrap::CompileOptions {
                source_dir: file.parent().map(Path::to_path_buf),
                providers: runinator_provider_catalog::metadata(),
                type_policy: (*typing).into(),
                workflow_signatures: runinator_pack::source::rexrap_context_workflow_signatures(
                    file,
                    Some(&source),
                )?,
                ..runinator_rexrap::CompileOptions::default()
            };
            let definition = runinator_rexrap::compile_str(&source, &options)
                .map_err(|e| err(e.render(&source)))?;
            if json_output {
                return output::json(&definition);
            }
            let rendered = serde_json::to_string_pretty(&definition)?;
            match output {
                Some(path) => {
                    fs::write(path, rendered)?;
                    println!("wrote {}", path.display());
                }
                None => println!("{rendered}"),
            }
        }
        RexRapCommands::Decompile {
            file,
            output,
            explicit,
        } => {
            let definition = read_workflow_definition(file)?;
            let options = runinator_rexrap::DecompileOptions {
                explicit: *explicit,
            };
            let source = runinator_rexrap::decompile_with(&definition, &options)
                .map_err(|e| err(e.to_string()))?;
            match output {
                Some(path) => {
                    fs::write(path, &source)?;
                    println!("wrote {}", path.display());
                }
                None => print!("{source}"),
            }
        }
        RexRapCommands::Format {
            file,
            output,
            check,
        } => {
            let source = fs::read_to_string(file)?;
            let formatted =
                runinator_rexrap::format_str(&source).map_err(|e| err(e.render(&source)))?;
            if *check {
                if formatted == source {
                    println!("{} ok", file.display());
                    return Ok(());
                }
                return Err(err(format!("{} is not formatted", file.display())));
            }
            match output {
                Some(path) => {
                    fs::write(path, formatted)?;
                    println!("wrote {}", path.display());
                }
                None => print!("{formatted}"),
            }
        }
        RexRapCommands::Check { file, typing } => {
            let source = fs::read_to_string(file)?;
            // analyze first so every error and warning is reported, not just the first.
            let providers = runinator_provider_catalog::metadata();
            let type_policy = (*typing).into();
            let workflow_signatures =
                runinator_pack::source::rexrap_context_workflow_signatures(file, Some(&source))?;
            let diagnostics = runinator_rexrap::analyze_source_with_options(
                &source,
                &providers,
                type_policy,
                &workflow_signatures,
            )
            .map_err(|e| err(e.render(&source)))?;
            let error_count = diagnostics.iter().filter(|d| d.is_error()).count();
            if json_output {
                return output::json(&json!({
                    "ok": error_count == 0,
                    "typing": typing.label(),
                    "diagnostics": diagnostics
                        .iter()
                        .map(|d| json!({
                            "severity": if d.is_error() { "error" } else { "warning" },
                            "message": d.message,
                            "start": d.span.start,
                            "end": d.span.end,
                        }))
                        .collect::<Vec<_>>(),
                }));
            }
            for diagnostic in &diagnostics {
                eprintln!("{}\n", diagnostic.render(&source));
            }
            if error_count > 0 {
                return Err(err(format!(
                    "{error_count} error(s) found in {}",
                    file.display()
                )));
            }
            // no errors: run the full compile (validator included) for the summary line.
            let options = runinator_rexrap::CompileOptions {
                source_dir: file.parent().map(Path::to_path_buf),
                providers,
                type_policy,
                workflow_signatures,
                ..runinator_rexrap::CompileOptions::default()
            };
            let definition = runinator_rexrap::compile_str(&source, &options)
                .map_err(|e| err(e.render(&source)))?;
            println!("{} v{} ok", definition.name, definition.version);
        }
    }
    Ok(())
}

impl CliTyping {
    fn label(self) -> &'static str {
        match self {
            CliTyping::Strict => "strict",
            CliTyping::Permissive => "permissive",
        }
    }
}
