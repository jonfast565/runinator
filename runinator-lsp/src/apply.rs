//! apply-on-save: compile the saved pack source client-side and import it into the running web
//! service, mirroring `runinatorctl workflows dev` minus the file watcher.

use std::path::Path;

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_pack::source;

/// compile and import the pack at `path` against `service_url`, returning a short status message.
pub async fn apply(service_url: &str, path: &Path) -> Result<String, String> {
    if !source::is_pack_source(path) {
        return Err(format!(
            "{} is not a pack source (.wdl/.wdlm/directory)",
            path.display()
        ));
    }

    // compile client-side; surface compile errors to the caller for diagnostics + a toast.
    let bundle = source::load_workflow_bundle(path).map_err(|err| err.to_string())?;
    let secrets = source::load_pack_settings(path).map_err(|err| err.to_string())?;
    let pipelines = source::load_pack_pipelines(path).map_err(|err| err.to_string())?;

    let client = AsyncApiClient::new(StaticLocator::new(service_url.to_string()))
        .map_err(|err| err.to_string())?;
    let result = client
        .import_pack(&bundle, secrets.as_ref(), pipelines.as_ref(), true)
        .await
        .map_err(|err| err.to_string())?;

    let count = result.workflows.workflows.len();
    Ok(format!(
        "imported {count} workflow{} into {service_url}",
        if count == 1 { "" } else { "s" }
    ))
}
