//! A cluster-backed mutex for every mutating `xtask k8s` command.
//!
//! The lock deliberately lives outside the `runinator` namespace so `k8s delete` cannot remove
//! its own coordination primitive while deletion is still in progress. Kubernetes' Lease object
//! gives a crashed process a bounded failure mode: a later deploy can take over after the holder
//! stops renewing rather than leaving a permanent lock behind.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};

const LOCK_NAMESPACE: &str = "runinator-deploy-lock";
const LOCK_NAME: &str = "runinator-xtask";
const LEASE_DURATION_SECS: i64 = 300;
const RENEW_INTERVAL: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub struct DeploymentLock {
    workspace_root: PathBuf,
    context_args: Vec<String>,
    holder_identity: String,
    stop_tx: Option<mpsc::Sender<()>>,
    heartbeat: Option<thread::JoinHandle<()>>,
    lost: Arc<AtomicBool>,
}

impl DeploymentLock {
    pub fn acquire(
        workspace_root: &Path,
        kube_context: Option<&str>,
        operation: &str,
        timeout: Duration,
    ) -> Result<Self> {
        let context_args = context_args(kube_context);
        ensure_lock_namespace(workspace_root, &context_args)?;

        let holder_identity = holder_identity(operation);
        let started = Instant::now();
        let mut announced_wait = false;

        loop {
            let now = Utc::now();
            match read_lease(workspace_root, &context_args)? {
                None => {
                    if try_create_lease(workspace_root, &context_args, &holder_identity, now)? {
                        break;
                    }
                }
                Some(lease) if lease_is_available(&lease, now) => {
                    if try_claim_lease(
                        workspace_root,
                        &context_args,
                        &lease,
                        &holder_identity,
                        now,
                    )? {
                        break;
                    }
                }
                Some(lease) => {
                    if !announced_wait {
                        let holder = lease_holder(&lease).unwrap_or("unknown holder");
                        println!("==> Waiting for Kubernetes deployment lock held by {holder}");
                        announced_wait = true;
                    }
                }
            }

            if started.elapsed() >= timeout {
                let holder = read_lease(workspace_root, &context_args)?
                    .and_then(|lease| lease_holder(&lease).map(str::to_owned))
                    .unwrap_or_else(|| "another contender".to_string());
                bail!(
                    "timed out after {}s waiting for Kubernetes deployment lock held by {holder}; retry later or increase --deploy-lock-timeout-secs",
                    timeout.as_secs()
                );
            }
            thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
        }

        println!("==> Acquired Kubernetes deployment lock as {holder_identity}");
        let lost = Arc::new(AtomicBool::new(false));
        let (stop_tx, stop_rx) = mpsc::channel();
        let heartbeat = spawn_heartbeat(
            workspace_root.to_path_buf(),
            context_args.clone(),
            holder_identity.clone(),
            Arc::clone(&lost),
            stop_rx,
        );

        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
            context_args,
            holder_identity,
            stop_tx: Some(stop_tx),
            heartbeat: Some(heartbeat),
            lost,
        })
    }

    pub fn ensure_held(&self) -> Result<()> {
        if self.lost.load(Ordering::Acquire) {
            bail!(
                "Kubernetes deployment lock ownership was lost during the operation; inspect the cluster before retrying"
            );
        }
        Ok(())
    }
}

impl Drop for DeploymentLock {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(heartbeat) = self.heartbeat.take() {
            let _ = heartbeat.join();
        }
        if let Err(err) = release_lease(
            &self.workspace_root,
            &self.context_args,
            &self.holder_identity,
        ) {
            eprintln!(
                "warning: failed to release Kubernetes deployment lock (it expires automatically): {err:#}"
            );
        } else {
            println!("==> Released Kubernetes deployment lock");
        }
    }
}

fn context_args(kube_context: Option<&str>) -> Vec<String> {
    match kube_context {
        Some(context) => vec!["--context".to_string(), context.to_string()],
        None => Vec::new(),
    }
}

fn holder_identity(operation: &str) -> String {
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string());
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("xtask/{operation}/{host}/{}/{nonce}", std::process::id())
}

fn timestamp(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn lease_holder(lease: &Value) -> Option<&str> {
    lease
        .pointer("/spec/holderIdentity")
        .and_then(Value::as_str)
        .filter(|holder| !holder.is_empty())
}

fn lease_is_available(lease: &Value, now: DateTime<Utc>) -> bool {
    if lease_holder(lease).is_none() {
        return true;
    }

    let duration = lease
        .pointer("/spec/leaseDurationSeconds")
        .and_then(Value::as_i64)
        .unwrap_or(LEASE_DURATION_SECS);
    let renewed = lease
        .pointer("/spec/renewTime")
        .or_else(|| lease.pointer("/spec/acquireTime"))
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));

    renewed.is_none_or(|renewed| renewed + chrono::Duration::seconds(duration) <= now)
}

fn ensure_lock_namespace(workspace_root: &Path, context_args: &[String]) -> Result<()> {
    let mut args = context_args.to_vec();
    args.extend([
        "get".to_string(),
        "namespace".to_string(),
        LOCK_NAMESPACE.to_string(),
        "--ignore-not-found".to_string(),
        "-o".to_string(),
        "name".to_string(),
    ]);
    let output = kubectl_output(workspace_root, &args, None)?;
    ensure_success(&args, &output)?;
    if !output.stdout.is_empty() {
        return Ok(());
    }

    let mut create_args = context_args.to_vec();
    create_args.extend([
        "create".to_string(),
        "namespace".to_string(),
        LOCK_NAMESPACE.to_string(),
    ]);
    let output = kubectl_output(workspace_root, &create_args, None)?;
    if output.status.success() || is_contention(&output) {
        Ok(())
    } else {
        command_error(&create_args, &output)
    }
}

fn read_lease(workspace_root: &Path, context_args: &[String]) -> Result<Option<Value>> {
    let mut args = context_args.to_vec();
    args.extend([
        "get".to_string(),
        "lease".to_string(),
        LOCK_NAME.to_string(),
        "--namespace".to_string(),
        LOCK_NAMESPACE.to_string(),
        "--ignore-not-found".to_string(),
        "-o".to_string(),
        "json".to_string(),
    ]);
    let output = kubectl_output(workspace_root, &args, None)?;
    ensure_success(&args, &output)?;
    if output.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    serde_json::from_slice(&output.stdout)
        .context("kubectl returned invalid JSON for the deployment Lease")
        .map(Some)
}

fn lease_document(holder_identity: &str, now: DateTime<Utc>) -> Value {
    let now = timestamp(now);
    json!({
        "apiVersion": "coordination.k8s.io/v1",
        "kind": "Lease",
        "metadata": {
            "name": LOCK_NAME,
            "namespace": LOCK_NAMESPACE,
            "labels": {
                "app.kubernetes.io/part-of": "runinator",
                "app.kubernetes.io/managed-by": "xtask"
            }
        },
        "spec": {
            "acquireTime": now,
            "holderIdentity": holder_identity,
            "leaseDurationSeconds": LEASE_DURATION_SECS,
            "leaseTransitions": 0,
            "renewTime": now
        }
    })
}

fn try_create_lease(
    workspace_root: &Path,
    context_args: &[String],
    holder_identity: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let mut args = context_args.to_vec();
    args.extend(["create".to_string(), "-f".to_string(), "-".to_string()]);
    let document = serde_json::to_vec(&lease_document(holder_identity, now))?;
    let output = kubectl_output(workspace_root, &args, Some(&document))?;
    contention_or_error(&args, &output)
}

fn try_claim_lease(
    workspace_root: &Path,
    context_args: &[String],
    current: &Value,
    holder_identity: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let resource_version = current
        .pointer("/metadata/resourceVersion")
        .and_then(Value::as_str)
        .context("deployment Lease has no metadata.resourceVersion")?;
    let transitions = current
        .pointer("/spec/leaseTransitions")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .saturating_add(1);
    let now = timestamp(now);
    let document = json!({
        "apiVersion": "coordination.k8s.io/v1",
        "kind": "Lease",
        "metadata": {
            "name": LOCK_NAME,
            "namespace": LOCK_NAMESPACE,
            "resourceVersion": resource_version,
            "labels": {
                "app.kubernetes.io/part-of": "runinator",
                "app.kubernetes.io/managed-by": "xtask"
            }
        },
        "spec": {
            "acquireTime": now,
            "holderIdentity": holder_identity,
            "leaseDurationSeconds": LEASE_DURATION_SECS,
            "leaseTransitions": transitions,
            "renewTime": now
        }
    });
    let mut args = context_args.to_vec();
    args.extend(["replace".to_string(), "-f".to_string(), "-".to_string()]);
    let document = serde_json::to_vec(&document)?;
    let output = kubectl_output(workspace_root, &args, Some(&document))?;
    contention_or_error(&args, &output)
}

fn spawn_heartbeat(
    workspace_root: PathBuf,
    context_args: Vec<String>,
    holder_identity: String,
    lost: Arc<AtomicBool>,
    stop_rx: mpsc::Receiver<()>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while stop_rx.recv_timeout(RENEW_INTERVAL).is_err() {
            match renew_lease(&workspace_root, &context_args, &holder_identity) {
                Ok(true) => {}
                Ok(false) => {
                    lost.store(true, Ordering::Release);
                    return;
                }
                Err(err) => {
                    eprintln!(
                        "warning: could not renew Kubernetes deployment lock; will retry: {err:#}"
                    );
                }
            }
        }
    })
}

fn renew_lease(
    workspace_root: &Path,
    context_args: &[String],
    holder_identity: &str,
) -> Result<bool> {
    let patch = json!([
        {"op": "test", "path": "/spec/holderIdentity", "value": holder_identity},
        {"op": "replace", "path": "/spec/renewTime", "value": timestamp(Utc::now())}
    ]);
    patch_lease(workspace_root, context_args, &patch)
}

fn release_lease(
    workspace_root: &Path,
    context_args: &[String],
    holder_identity: &str,
) -> Result<()> {
    let patch = json!([
        {"op": "test", "path": "/spec/holderIdentity", "value": holder_identity},
        {"op": "replace", "path": "/spec/holderIdentity", "value": ""},
        {"op": "replace", "path": "/spec/renewTime", "value": timestamp(Utc::now())}
    ]);
    if patch_lease(workspace_root, context_args, &patch)? {
        Ok(())
    } else {
        bail!("deployment Lease is no longer held by this process")
    }
}

fn patch_lease(workspace_root: &Path, context_args: &[String], patch: &Value) -> Result<bool> {
    let mut args = context_args.to_vec();
    args.extend([
        "patch".to_string(),
        "lease".to_string(),
        LOCK_NAME.to_string(),
        "--namespace".to_string(),
        LOCK_NAMESPACE.to_string(),
        "--type=json".to_string(),
        "--patch".to_string(),
        serde_json::to_string(patch)?,
    ]);
    let output = kubectl_output(workspace_root, &args, None)?;
    if output.status.success() {
        return Ok(true);
    }

    // Kubernetes' wording for a failed JSON Patch `test` varies by server version. Read the
    // authoritative holder instead of classifying stderr text: a missing Lease or a different
    // holder proves ownership was lost, while an unchanged holder means this was an operational
    // failure that the heartbeat can retry.
    match read_lease(workspace_root, context_args)? {
        None => Ok(false),
        Some(lease) if lease_holder(&lease) != patch_holder_identity(patch) => Ok(false),
        Some(_) => command_error(&args, &output),
    }
}

fn patch_holder_identity(patch: &Value) -> Option<&str> {
    patch
        .as_array()
        .and_then(|operations| operations.first())
        .and_then(|operation| operation.get("value"))
        .and_then(Value::as_str)
}

fn kubectl_output(workspace_root: &Path, args: &[String], stdin: Option<&[u8]>) -> Result<Output> {
    let mut command = Command::new("kubectl");
    command
        .args(args)
        .current_dir(workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn 'kubectl {}'", args.join(" ")))?;
    if let Some(stdin) = stdin {
        child
            .stdin
            .take()
            .context("kubectl stdin was not piped")?
            .write_all(stdin)?;
    }
    child
        .wait_with_output()
        .with_context(|| format!("failed to wait for 'kubectl {}'", args.join(" ")))
}

fn contention_or_error(args: &[String], output: &Output) -> Result<bool> {
    if output.status.success() {
        Ok(true)
    } else if is_contention(output) {
        Ok(false)
    } else {
        command_error(args, output)
    }
}

fn is_contention(output: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.contains("AlreadyExists")
        || stderr.contains("already exists")
        || stderr.contains("Conflict")
        || stderr.contains("the object has been modified")
}

fn ensure_success(args: &[String], output: &Output) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        command_error(args, output)
    }
}

fn command_error<T>(args: &[String], output: &Output) -> Result<T> {
    bail!(
        "command 'kubectl {}' failed with {}: {}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

#[cfg(test)]
mod tests {
    use super::{LEASE_DURATION_SECS, lease_document, lease_is_available};
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    #[test]
    fn held_lease_is_unavailable_until_its_duration_elapses() {
        let renewed = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
        let lease = lease_document("holder", renewed);

        assert!(!lease_is_available(
            &lease,
            renewed + Duration::seconds(LEASE_DURATION_SECS - 1)
        ));
        assert!(lease_is_available(
            &lease,
            renewed + Duration::seconds(LEASE_DURATION_SECS)
        ));
    }

    #[test]
    fn released_or_malformed_lease_is_available() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
        assert!(lease_is_available(
            &json!({"spec": {"holderIdentity": "", "renewTime": "not-a-time"}}),
            now
        ));
        assert!(lease_is_available(
            &json!({"spec": {"holderIdentity": "stale", "renewTime": "not-a-time"}}),
            now
        ));
    }

    #[test]
    fn lease_document_carries_owner_and_expiry_contract() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
        let lease = lease_document("xtask/deploy/test", now);

        assert_eq!(
            lease
                .pointer("/spec/holderIdentity")
                .and_then(|v| v.as_str()),
            Some("xtask/deploy/test")
        );
        assert_eq!(
            lease
                .pointer("/spec/leaseDurationSeconds")
                .and_then(|v| v.as_i64()),
            Some(LEASE_DURATION_SECS)
        );
        assert_eq!(
            lease
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str()),
            Some("runinator-deploy-lock")
        );
    }
}
