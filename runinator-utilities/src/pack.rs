// shared layout for the compiled workflow pack uploaded to the web service. the client compiles a
// pack (`.wdl`/`.wdls`/`.wdlp`) and zips the resulting json artifacts; the web service unzips and
// imports them. compilation stays on the client — the backend only reads the compiled json here.

use std::io::{Cursor, Read, Write};

use runinator_models::bundles::SecretBundle;
use runinator_models::workflows::WorkflowBundle;
use zip::write::SimpleFileOptions;

/// zip entry holding the compiled `WorkflowBundle` json (always present).
pub const WORKFLOWS_ENTRY: &str = "workflows.json";
/// zip entry holding the compiled `SecretBundle` json (optional).
pub const SECRETS_ENTRY: &str = "secrets.json";

/// error type for pack zip read/write; boxes zip and serde failures alike.
pub type PackError = Box<dyn std::error::Error + Send + Sync>;

/// build a compiled pack zip from a workflow bundle and an optional secret bundle.
pub fn build_pack_zip(
    workflows: &WorkflowBundle,
    secrets: Option<&SecretBundle>,
) -> Result<Vec<u8>, PackError> {
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        // stored (uncompressed) keeps the zip backend dependency-free and these payloads small.
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file(WORKFLOWS_ENTRY, options)?;
        zip.write_all(&serde_json::to_vec(workflows)?)?;
        if let Some(secrets) = secrets {
            zip.start_file(SECRETS_ENTRY, options)?;
            zip.write_all(&serde_json::to_vec(secrets)?)?;
        }
        zip.finish()?;
    }
    Ok(buffer)
}

/// read a compiled pack zip back into its workflow bundle and optional secret bundle.
pub fn read_pack_zip(bytes: &[u8]) -> Result<(WorkflowBundle, Option<SecretBundle>), PackError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let workflows: WorkflowBundle = {
        let mut file = archive
            .by_name(WORKFLOWS_ENTRY)
            .map_err(|_| format!("pack zip missing '{WORKFLOWS_ENTRY}'"))?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        serde_json::from_str(&text)?
    };
    let secrets = match archive.by_name(SECRETS_ENTRY) {
        Ok(mut file) => {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            Some(serde_json::from_str(&text)?)
        }
        Err(zip::result::ZipError::FileNotFound) => None,
        Err(err) => return Err(err.into()),
    };
    Ok((workflows, secrets))
}

#[cfg(test)]
mod tests {
    use super::*;
    use runinator_models::bundles::{SecretBundle, SecretBundleEntry};
    use runinator_models::settings::SettingKind;
    use runinator_models::value::Value;
    use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition};

    #[test]
    fn pack_zip_round_trips() {
        let workflows = WorkflowBundle {
            workflows: vec![WorkflowDefinition {
                id: None,
                name: "demo".into(),
                version: runinator_models::semver::SemVer::new(1, 0, 0),
                enabled: true,
                input_type: Default::default(),
                definition: Default::default(),
                created_at: None,
                updated_at: None,
            }],
            triggers: Vec::new(),
        };
        let secrets = SecretBundle {
            secrets: vec![SecretBundleEntry {
                scope: "jira".into(),
                name: "token".into(),
                value: Value::from("abc"),
                schema: None,
                kind: SettingKind::Secret,
                updated_at: None,
            }],
        };

        let zipped = build_pack_zip(&workflows, Some(&secrets)).expect("zip");
        let (read_workflows, read_secrets) = read_pack_zip(&zipped).expect("unzip");
        assert_eq!(read_workflows.workflows.len(), 1);
        assert_eq!(read_workflows.workflows[0].name, "demo");
        let read_secrets = read_secrets.expect("secrets present");
        assert_eq!(read_secrets.secrets, secrets.secrets);

        // secrets are optional.
        let (_, none_secrets) =
            read_pack_zip(&build_pack_zip(&workflows, None).expect("zip")).expect("unzip");
        assert!(none_secrets.is_none());
    }
}
