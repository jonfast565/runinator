// shared layout for the compiled workflow pack uploaded to the web service. the client compiles a
// pack (`.wdl`/`.wdls`/`.wdlm`) and zips the resulting json artifacts; the web service unzips and
// imports them. compilation stays on the client — the backend only reads the compiled json here.

use std::io::{Cursor, Read, Write};

use runinator_models::bundles::SecretBundle;
use runinator_models::pipelines::PipelineBundle;
use runinator_models::workflows::WorkflowBundle;
use zip::write::SimpleFileOptions;

/// zip entry holding the compiled `WorkflowBundle` json (always present).
pub const WORKFLOWS_ENTRY: &str = "workflows.json";
/// zip entry holding the compiled `SecretBundle` json (optional).
pub const SECRETS_ENTRY: &str = "secrets.json";
/// zip entry holding the compiled `PipelineBundle` json (optional).
pub const PIPELINES_ENTRY: &str = "pipelines.json";

/// error type for pack zip read/write; boxes zip and serde failures alike.
pub type PackError = Box<dyn std::error::Error + Send + Sync>;

/// what a pack zip carries once read back: workflows plus optional secrets and pipelines.
pub struct PackContents {
    pub workflows: WorkflowBundle,
    pub secrets: Option<SecretBundle>,
    pub pipelines: Option<PipelineBundle>,
}

/// build a compiled pack zip from a workflow bundle and optional secret / pipeline bundles.
pub fn build_pack_zip(
    workflows: &WorkflowBundle,
    secrets: Option<&SecretBundle>,
    pipelines: Option<&PipelineBundle>,
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
        if let Some(pipelines) = pipelines.filter(|p| !p.pipelines.is_empty()) {
            zip.start_file(PIPELINES_ENTRY, options)?;
            zip.write_all(&serde_json::to_vec(pipelines)?)?;
        }
        zip.finish()?;
    }
    Ok(buffer)
}

/// read a compiled pack zip back into its workflow bundle and optional secret / pipeline bundles.
pub fn read_pack_zip(bytes: &[u8]) -> Result<PackContents, PackError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let workflows: WorkflowBundle = {
        let mut file = archive
            .by_name(WORKFLOWS_ENTRY)
            .map_err(|_| format!("pack zip missing '{WORKFLOWS_ENTRY}'"))?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        serde_json::from_str(&text)?
    };
    let secrets = read_optional_entry(&mut archive, SECRETS_ENTRY)?;
    let pipelines = read_optional_entry(&mut archive, PIPELINES_ENTRY)?;
    Ok(PackContents {
        workflows,
        secrets,
        pipelines,
    })
}

// read and deserialize an optional named entry, returning None when the entry is absent.
fn read_optional_entry<T: serde::de::DeserializeOwned>(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<T>, PackError> {
    match archive.by_name(name) {
        Ok(mut file) => {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            Ok(Some(serde_json::from_str(&text)?))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runinator_models::bundles::{SecretBundle, SecretBundleEntry};
    use runinator_models::pipelines::{PipelineLinkSelector, PipelineLinkSpec, PipelineSpec};
    use runinator_models::settings::SettingKind;
    use runinator_models::value::Value;
    use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition};

    #[test]
    fn pack_zip_round_trips() {
        let workflows = WorkflowBundle {
            workflows: vec![WorkflowDefinition {
                id: None,
                name: "demo".into(),
                namespace: None,
                org_id: None,
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

        let pipelines = PipelineBundle {
            pipelines: vec![PipelineSpec {
                name: "Core SDLC".into(),
                description: Some("demo pipeline".into()),
                defaults: Default::default(),
                members: vec!["demo".into()],
                links: vec![PipelineLinkSpec {
                    from: "demo".into(),
                    to: "demo".into(),
                    on: PipelineLinkSelector::Complete,
                    enabled: true,
                }],
                triggers: vec![],
            }],
        };

        let zipped = build_pack_zip(&workflows, Some(&secrets), Some(&pipelines)).expect("zip");
        let contents = read_pack_zip(&zipped).expect("unzip");
        assert_eq!(contents.workflows.workflows.len(), 1);
        assert_eq!(contents.workflows.workflows[0].name, "demo");
        let read_secrets = contents.secrets.expect("secrets present");
        assert_eq!(read_secrets.secrets, secrets.secrets);
        assert_eq!(contents.pipelines.expect("pipelines present"), pipelines);

        // secrets and pipelines are optional.
        let contents =
            read_pack_zip(&build_pack_zip(&workflows, None, None).expect("zip")).expect("unzip");
        assert!(contents.secrets.is_none());
        assert!(contents.pipelines.is_none());
    }
}
