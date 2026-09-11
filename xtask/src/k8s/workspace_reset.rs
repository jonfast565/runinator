//! Restartable, workspace-only cutover for the repository's PostgreSQL/FsBlob cluster.
use crate::exec;
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use std::path::Path;

const LEDGER: &str = "runinator-workspace-storage-cutover";
const PAUSE: &[&str] = &[
    "runinator-ws",
    "runinator-engine-worker",
    "runinator-worker",
    "runinator-adapter-host",
    "runinator-archiver",
];

fn args(context: Option<&str>, tail: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(context) = context {
        out.extend(["--context".into(), context.into()]);
    }
    out.extend(["-n".into(), "runinator".into()]);
    out.extend(tail.iter().map(|s| (*s).into()));
    out
}
fn capture(root: &Path, context: Option<&str>, tail: &[&str]) -> Result<String> {
    let owned = args(context, tail);
    exec::capture(
        "kubectl",
        &owned.iter().map(String::as_str).collect::<Vec<_>>(),
        root,
    )
}
fn run(root: &Path, context: Option<&str>, tail: &[&str]) -> Result<()> {
    let owned = args(context, tail);
    exec::run(
        "kubectl",
        &owned.iter().map(String::as_str).collect::<Vec<_>>(),
        root,
    )
}
fn ledger(root: &Path, context: Option<&str>, state: &str, replicas: &Value) -> Result<()> {
    let document = json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"name":LEDGER,"namespace":"runinator"},"data":{"state":state,"replicas":serde_json::to_string(replicas)?}});
    let owned = args(context, &["apply", "-f", "-"]);
    exec::run_with_stdin(
        "kubectl",
        &owned.iter().map(String::as_str).collect::<Vec<_>>(),
        root,
        &document.to_string(),
    )
}
fn sql(root: &Path, context: Option<&str>, query: &str) -> Result<String> {
    capture(
        root,
        context,
        &[
            "exec",
            "statefulset/runinator-postgres",
            "--",
            "sh",
            "-c",
            "exec psql -X -qAt -v ON_ERROR_STOP=1 -U \"$POSTGRES_USER\" -d \"$POSTGRES_DB\" -c \"$1\"",
            "workspace-reset",
            query,
        ],
    )
}
fn active(root: &Path, context: Option<&str>) -> Result<String> {
    sql(
        root,
        context,
        "WITH affected AS (SELECT DISTINCT r.id, r.pipeline_run_id, r.finished_at FROM workflow_runs r JOIN (SELECT c.workflow_run_id FROM workspace_checkouts w JOIN workflow_effects e ON e.id = w.effect_id JOIN workflow_continuations c ON c.id = e.continuation_id UNION SELECT workflow_run_id FROM workspace_pins UNION SELECT workflow_run_id FROM workspace_snapshots) w ON w.workflow_run_id = r.id) SELECT 'workflow ' || id::text FROM affected WHERE finished_at IS NULL UNION SELECT 'pipeline ' || p.id::text FROM pipeline_runs p JOIN affected a ON a.pipeline_run_id = p.id WHERE p.finished_at IS NULL ORDER BY 1",
    )
}
fn resume(root: &Path, context: Option<&str>, replicas: &Value) -> Result<()> {
    if let Some(items) = replicas.as_object() {
        for (name, count) in items {
            if !PAUSE.contains(&name.as_str()) {
                continue;
            }

            run(
                root,
                context,
                &[
                    "scale",
                    &format!("deployment/{name}"),
                    &format!("--replicas={}", count.as_u64().unwrap_or(0)),
                ],
            )?;
        }
    }
    Ok(())
}

pub fn reset(root: &Path, context: Option<&str>, restore: bool) -> Result<()> {
    let previous = capture(
        root,
        context,
        &[
            "get",
            "configmap",
            LEDGER,
            "--ignore-not-found",
            "-o",
            "json",
        ],
    )?;
    let previous: Value = if previous.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&previous)?
    };
    let replicas = if let Some(saved) = previous.pointer("/data/replicas").and_then(Value::as_str) {
        serde_json::from_str(saved)?
    } else {
        let deployments: Value = serde_json::from_str(&capture(
            root,
            context,
            &["get", "deployments", "-o", "json"],
        )?)?;
        let mut replicas = serde_json::Map::new();
        for item in deployments["items"].as_array().into_iter().flatten() {
            let Some(name) = item
                .pointer("/metadata/name")
                .and_then(Value::as_str)
                .filter(|name| PAUSE.contains(name))
            else {
                continue;
            };

            replicas.insert(
                name.into(),
                item.pointer("/spec/replicas").cloned().unwrap_or(json!(1)),
            );
        }
        Value::Object(replicas)
    };
    if restore {
        resume(root, context, &replicas)?;
        return ledger(root, context, "resumed", &replicas);
    }
    if matches!(
        previous.pointer("/data/state").and_then(Value::as_str),
        Some("empty" | "resumed")
    ) {
        bail!("cutover already completed; refusing to erase workspaces created after cutover");
    }
    let common: Value = serde_json::from_str(&capture(
        root,
        context,
        &["get", "configmap", "runinator-common", "-o", "json"],
    )?)?;
    ensure!(
        common
            .pointer("/data/RUNINATOR_BLOB_ENDPOINT")
            .and_then(Value::as_str)
            == Some("http://runinator-blob.runinator.svc.cluster.local:9000/"),
        "workspace reset supports only the repository's FsBlob deployment; external object stores require their own scoped cutover"
    );
    let waiting = active(root, context)?;
    ensure!(
        waiting.trim().is_empty(),
        "cancel these workspace-dependent runs through the ordinary API before cutover:\n{waiting}"
    );
    ledger(root, context, "quiescing", &replicas)?;
    if let Some(items) = replicas.as_object() {
        for name in items.keys() {
            run(
                root,
                context,
                &["scale", &format!("deployment/{name}"), "--replicas=0"],
            )?;
        }
        for name in items.keys() {
            run(
                root,
                context,
                &[
                    "wait",
                    "--for=delete",
                    "pod",
                    "-l",
                    &format!("app={name}"),
                    "--timeout=180s",
                ],
            )?;
        }
    }
    let waiting = active(root, context)?;
    if !waiting.trim().is_empty() {
        resume(root, context, &replicas)?;
        bail!(
            "new workspace runs arrived during quiescence; deployments restored. Cancel these runs and retry:\n{waiting}"
        );
    }
    ledger(root, context, "quiesced", &replicas)?;
    let query = r#"BEGIN;
DO $reset$ DECLARE item text; BEGIN
FOR item IN SELECT unnest(ARRAY['workspace_gc_objects','workspace_gc_state','workspace_retired_packs','workspace_readers','workspace_downloads','workspace_receipts','workspace_objects','workspace_transfers','workspace_pins','workspace_checkouts','workspace_snapshots']) LOOP
IF to_regclass(item) IS NOT NULL THEN EXECUTE format('DELETE FROM %I', item); END IF;
END LOOP;
END $reset$;
DELETE FROM resource_grants WHERE resource_type = 'workspace';
DELETE FROM resource_ownership WHERE resource_type = 'workspace';
DELETE FROM durable_workspaces;
COMMIT;
SELECT count(*) FROM durable_workspaces;"#;
    ensure!(
        sql(root, context, query)?.trim() == "0",
        "workspace reset did not produce an empty registry"
    );
    ledger(root, context, "metadata-cleared", &replicas)?;
    // only the four trees in the durable workspace bucket; retain its bucket marker and every sibling bucket.
    run(
        root,
        context,
        &[
            "exec",
            "deployment/runinator-blob",
            "--",
            "sh",
            "-c",
            "test \"$RUNINATOR_BLOB_DATA_DIR\" = /var/lib/runinator/blobs && rm -rf /var/lib/runinator/blobs/runinator-workspaces/objects /var/lib/runinator/blobs/runinator-workspaces/data /var/lib/runinator/blobs/runinator-workspaces/meta /var/lib/runinator/blobs/runinator-workspaces/uploads /var/lib/runinator/blobs/runinator-workspaces/.tmp",
        ],
    )?;
    ledger(root, context, "empty", &replicas)?;
    println!(
        "Workspace registry and bucket are empty. Deploy the new storage release, then use reset-workspaces --resume to restore the recorded replica counts."
    );
    Ok(())
}
