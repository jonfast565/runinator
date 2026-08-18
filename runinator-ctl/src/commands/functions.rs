use super::*;

use runinator_models::functions::{FunctionCatalogEntry, FunctionPackage, FunctionPackageDetail};
use runinator_pack::functions::FunctionSource;

use crate::cli::FunctionCommands;

pub(super) async fn functions(
    client: &Client,
    command: &FunctionCommands,
    json_output: bool,
) -> Result<()> {
    match command {
        // validate is intercepted before dispatch (it is offline); reaching here means that wiring
        // changed, so report it rather than silently contacting the web service.
        FunctionCommands::Validate { .. } => Err(err(
            "functions validate must be handled before command dispatch",
        )),
        FunctionCommands::Invoke {
            target,
            alias,
            version,
            input,
            input_file,
        } => {
            let (package, export) = target
                .rsplit_once('.')
                .ok_or_else(|| err("the function target must be package.export"))?;
            let input = match (input, input_file) {
                (Some(_), Some(_)) => return Err(err("pass --input or --input-file, not both")),
                (Some(text), None) => serde_json::from_str::<Value>(text)
                    .map_err(|e| err(format!("--input must be valid json: {e}")))?,
                (None, Some(path)) => params::load_json_file(path)?,
                (None, None) => Value::Object(Map::new()),
            };
            let result = client
                .invoke_function(package, export, alias.as_deref(), *version, &input)
                .await?;
            if json_output {
                return output::json(&result);
            }
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        FunctionCommands::List => {
            let packages = client.fetch_function_packages().await?;
            if json_output {
                return output::json(&packages);
            }
            print_packages(&packages);
            Ok(())
        }
        FunctionCommands::Show { package } => {
            let detail = client.fetch_function_package(package).await?;
            if json_output {
                return output::json(&detail);
            }
            print_package(&detail);
            Ok(())
        }
        FunctionCommands::Catalog => {
            let entries = client.fetch_function_catalog().await?;
            if json_output {
                return output::json(&entries);
            }
            print_catalog(&entries);
            Ok(())
        }
        FunctionCommands::Publish { path, alias } => {
            publish(client, path, alias.as_deref(), json_output).await
        }
        FunctionCommands::Versions { package } => {
            let detail = client.fetch_function_package(package).await?;
            if json_output {
                return output::json(&detail.versions);
            }
            print_versions(&detail);
            Ok(())
        }
        FunctionCommands::Alias {
            package,
            alias,
            version,
            from,
        } => {
            let moved = client
                .set_function_alias(package, alias, *version, from.as_deref())
                .await?;
            if json_output {
                return output::json(&moved);
            }
            println!("{} -> version {}", moved.name, moved.version);
            Ok(())
        }
        FunctionCommands::Unalias { package, alias } => {
            let result = client.delete_function_alias(package, alias).await?;
            if json_output {
                return output::json(&result);
            }
            println!("deleted alias {alias}");
            Ok(())
        }
        FunctionCommands::Delete { package } => {
            let result = client.delete_function_package(package).await?;
            if json_output {
                return output::json(&result);
            }
            println!("archived package {package}");
            Ok(())
        }
        FunctionCommands::Restore { package } => {
            let result = client.restore_function_package(package).await?;
            if json_output {
                return output::json(&result);
            }
            println!("restored package {package}");
            Ok(())
        }
    }
}

/// compile and check a package without contacting the web service.
///
/// offline so a package can be checked in a pre-commit hook or a build with no server reachable,
/// and so the digest a publish would upload can be seen before publishing anything.
pub fn functions_validate(path: &Path, json_output: bool) -> Result<()> {
    let source = FunctionSource::load(path).map_err(|error| err(error.to_string()))?;
    if json_output {
        return output::json(&json!({
            "package": source.qualified_name(),
            "digest": source.archive.digest,
            "size_bytes": source.archive.size_bytes(),
            "files": source.archive.files,
            "exports": source
                .manifest
                .sorted_exports()
                .iter()
                .map(|export| export.name.clone())
                .collect::<Vec<_>>(),
        }));
    }
    println!("package: {}", source.qualified_name());
    println!("runtime: {}", source.manifest.runtime.runtime);
    println!("digest:  {}", source.archive.digest);
    println!(
        "archive: {} files, {} bytes",
        source.archive.files.len(),
        source.archive.size_bytes()
    );
    println!();
    println!("{:<24} handler", "export");
    for export in source.manifest.sorted_exports() {
        println!(
            "{:<24} {}",
            output::truncate(&export.name, 24),
            export.handler
        );
    }
    Ok(())
}

// archive the directory, upload the bytes only if the server does not already hold them, then
// publish. the probe is the whole reason the digest is computed client-side: an unchanged package
// republishes without moving its bytes over the wire again.
async fn publish(
    client: &Client,
    path: &Path,
    alias: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let source = FunctionSource::load(path).map_err(|error| err(error.to_string()))?;
    let digest = source.archive.digest.clone();

    let existing = client.fetch_function_artifact(&digest).await?;
    let uploaded = match existing {
        Some(_) => false,
        None => {
            client
                .upload_function_artifact(&digest, source.archive.bytes.clone())
                .await?;
            true
        }
    };

    let mut request = source.publish_request();
    if let Some(alias) = alias {
        request.alias = Some(alias.to_string());
    }
    let version = client.publish_function_version(&request).await?;

    if json_output {
        return output::json(&json!({
            "package": source.qualified_name(),
            "version": version.version,
            "digest": digest,
            "artifact_uploaded": uploaded,
            "alias": request.alias,
        }));
    }
    println!(
        "published {} version {} ({})",
        source.qualified_name(),
        version.version,
        if uploaded {
            "artifact uploaded"
        } else {
            "artifact already stored"
        }
    );
    if let Some(alias) = &request.alias {
        println!("alias {alias} -> version {}", version.version);
    }
    Ok(())
}

fn print_packages(packages: &[FunctionPackage]) {
    println!("{:<32} {:>8} description", "package", "latest");
    for package in packages {
        println!(
            "{:<32} {:>8} {}",
            output::truncate(&package.qualified_name(), 32),
            package
                .latest_version
                .map(|version| version.to_string())
                .unwrap_or_else(|| "-".into()),
            output::truncate(package.description.as_deref().unwrap_or("-"), 40)
        );
    }
}

fn print_package(detail: &FunctionPackageDetail) {
    println!("package: {}", detail.package.qualified_name());
    if let Some(description) = &detail.package.description {
        println!("description: {description}");
    }
    println!("versions: {}", detail.versions.len());
    if !detail.aliases.is_empty() {
        let aliases = detail
            .aliases
            .iter()
            .map(|alias| format!("{}->{}", alias.name, alias.version))
            .collect::<Vec<_>>()
            .join(", ");
        println!("aliases: {aliases}");
    }
    println!();
    println!("{:<24} handler", "export");
    for export in &detail.exports {
        println!(
            "{:<24} {}",
            output::truncate(&export.name, 24),
            export.handler
        );
    }
}

fn print_versions(detail: &FunctionPackageDetail) {
    println!(
        "{:>8} {:<20} {:<28} published",
        "version", "runtime", "digest"
    );
    for version in &detail.versions {
        let aliases = detail
            .aliases
            .iter()
            .filter(|alias| alias.version_id == version.id)
            .map(|alias| alias.name.clone())
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{:>8} {:<20} {:<28} {} {}",
            version.version,
            output::truncate(&version.runtime.runtime, 20),
            output::truncate(&version.artifact_digest, 28),
            output::time(Some(version.created_at)),
            aliases
        );
    }
}

fn print_catalog(entries: &[FunctionCatalogEntry]) {
    println!("{:<40} {:>8} aliases", "call", "version");
    for entry in entries {
        println!(
            "{:<40} {:>8} {}",
            output::truncate(&entry.binding().call_path(), 40),
            entry.version,
            entry.aliases.join(",")
        );
    }
}
