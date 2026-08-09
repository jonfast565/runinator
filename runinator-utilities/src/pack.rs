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
#[path = "pack_tests.rs"]
mod tests;
