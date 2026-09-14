//! starts the local dev stack as a modular monolith by default, with the process supervisor kept
//! as an explicit compatibility topology.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::exec;
use crate::paths::ensure_dir;
use crate::platform::{executable_name, plugin_library_name};

pub struct LocalStackOptions<'a> {
    pub topology: LocalTopology,
    pub database_backend: &'a str,
    pub database_path: &'a Path,
    pub database_url: Option<&'a str>,
}

#[derive(Clone, Copy)]
pub enum LocalTopology {
    Standalone,
    Supervisor,
}

/// the checked-in supervisor config doesn't pass `--dll-path` to the worker, so it only looks in
/// `~/.runinator/plugins` (`runinator-worker`'s own default). copy the freshly built console
/// plugin there so the console provider actually loads, mirroring what the old artifact-publish
/// step used to guarantee.
fn ensure_console_plugin_installed(target_dir: &Path) -> Result<()> {
    let plugin_file_name = plugin_library_name();
    let source = target_dir.join(plugin_file_name);
    if !source.exists() {
        eprintln!(
            "warning: plugin library not found at {}. The worker will likely fail to load the console provider.",
            source.display()
        );
        return Ok(());
    }

    let plugins_dir = runinator_platform::app_data::app_data_path("plugins")
        .map_err(|err| anyhow::anyhow!("failed to resolve plugin directory: {err}"))?;
    ensure_dir(&plugins_dir)?;
    let destination = plugins_dir.join(plugin_file_name);

    let source_modified = std::fs::metadata(&source)
        .and_then(|meta| meta.modified())
        .ok();
    let dest_modified = std::fs::metadata(&destination)
        .and_then(|meta| meta.modified())
        .ok();
    let needs_copy = match (source_modified, dest_modified) {
        (Some(source_modified), Some(dest_modified)) => source_modified > dest_modified,
        _ => true,
    };

    if needs_copy {
        std::fs::copy(&source, &destination).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source.display(),
                destination.display()
            )
        })?;
    }
    Ok(())
}

/// starts the selected local topology in the foreground.
pub fn start_local_stack(
    workspace_root: &Path,
    target_dir: &Path,
    options: &LocalStackOptions,
) -> Result<()> {
    let binary_name = match options.topology {
        LocalTopology::Standalone => "runinator-standalone",
        LocalTopology::Supervisor => "runinator-supervisor",
    };
    let binary = target_dir.join(executable_name(binary_name));
    if !binary.exists() {
        bail!(
            "{binary_name} binary was not found at {}. Build the workspace first.",
            binary.display()
        );
    }
    ensure_console_plugin_installed(target_dir)?;

    let mut envs: Vec<(&str, String)> =
        vec![("RUNINATOR_DATABASE", options.database_backend.to_string())];
    if options.database_backend == "sqlite" {
        envs.push((
            "RUNINATOR_SQLITE_PATH",
            options.database_path.display().to_string(),
        ));
    } else {
        let database_url = options
            .database_url
            .filter(|url| !url.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("--database-url is required when --database is not sqlite")
            })?;
        envs.push(("RUNINATOR_DATABASE_URL", database_url.to_string()));
    }

    let binary_str = binary.display().to_string();
    let env_refs: Vec<(&str, &str)> = envs
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    match options.topology {
        LocalTopology::Standalone => {
            println!("Starting local Runinator stack via runinator-standalone");
            exec::run_with_env(
                &binary_str,
                &["start", "--foreground"],
                workspace_root,
                &env_refs,
            )
        }
        LocalTopology::Supervisor => {
            let config_path = workspace_root.join("runinator-supervisor.json");
            if !config_path.exists() {
                bail!("supervisor config not found at {}.", config_path.display());
            }
            let config_path_str = config_path.display().to_string();
            println!("Starting local Runinator stack via supervisor config '{config_path_str}'");
            exec::run_with_env(
                &binary_str,
                &["--config", &config_path_str, "start", "--foreground"],
                workspace_root,
                &env_refs,
            )
        }
    }
}
