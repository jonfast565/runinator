//! orchestrates a kustomize-based apply/delete against a running cluster: renders image overrides
//! into a disposable overlay copy, preserves already-running postgres/rabbitmq state unless asked
//! to recreate it, cleans up resources that were superseded by earlier renames, and waits for
//! rollouts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_yaml::Value;

use super::kustomize;
use super::yaml_docs;
use crate::exec;

pub struct DeployOptions<'a> {
    pub workspace_root: &'a Path,
    pub manifest_path: &'a Path,
    pub kube_context: Option<&'a str>,
    pub image_map: Option<HashMap<String, String>>,
    pub delete: bool,
    pub command_center_only: bool,
    pub recreate_infra: bool,
    pub expose_direct_ingress: bool,
}

pub struct GrafanaRedeployOptions<'a> {
    pub workspace_root: &'a Path,
    pub manifest_path: &'a Path,
    pub kube_context: Option<&'a str>,
}

pub struct DatabaseRedeployOptions<'a> {
    pub workspace_root: &'a Path,
    pub manifest_path: &'a Path,
    pub kube_context: Option<&'a str>,
    pub from_scratch: bool,
}

const STALE_RESOURCES: &[&str] = &[
    "deployment/runinator-importer",
    "job/runinator-importer",
    "job/runinator-pack-import",
    "service/runinator-gossip",
];

const NAMESPACE: &str = "runinator";

fn context_args(kube_context: Option<&str>) -> Vec<String> {
    match kube_context {
        Some(context) => vec!["--context".to_string(), context.to_string()],
        None => Vec::new(),
    }
}

fn kubectl_args<'a>(ctx_args: &'a [String], rest: &[&'a str]) -> Vec<&'a str> {
    ctx_args
        .iter()
        .map(String::as_str)
        .chain(rest.iter().copied())
        .collect()
}

fn resource_exists(workspace_root: &Path, ctx_args: &[String], kind: &str, name: &str) -> bool {
    let args = kubectl_args(
        ctx_args,
        &[
            "get",
            kind,
            name,
            "--namespace",
            NAMESPACE,
            "--ignore-not-found",
            "-o",
            "name",
        ],
    );
    !exec::capture_allow_failure("kubectl", &args, workspace_root)
        .trim()
        .is_empty()
}

fn kustomize_render(
    workspace_root: &Path,
    ctx_args: &[String],
    overlay_path: &Path,
) -> Result<String> {
    let overlay_path_str = overlay_path.display().to_string();
    let args = kubectl_args(ctx_args, &["kustomize", &overlay_path_str]);
    exec::capture("kubectl", &args, workspace_root)
}

fn render_manifest(
    workspace_root: &Path,
    ctx_args: &[String],
    apply_path: &Path,
    is_overlay: bool,
) -> Result<String> {
    if is_overlay {
        kustomize_render(workspace_root, ctx_args, apply_path)
    } else {
        Ok(std::fs::read_to_string(apply_path)?)
    }
}

pub fn deploy_kubernetes_stack(options: DeployOptions) -> Result<()> {
    exec::require_tool("kubectl")?;

    let resolved_path = if options.manifest_path.is_absolute() {
        options.manifest_path.to_path_buf()
    } else {
        options.workspace_root.join(options.manifest_path)
    };
    anyhow::ensure!(
        resolved_path.exists(),
        "kubernetes manifest or overlay not found at {}",
        resolved_path.display()
    );
    let is_overlay = resolved_path.is_dir();

    let mut apply_path: PathBuf = resolved_path.clone();
    if is_overlay && (options.image_map.is_some() || options.expose_direct_ingress) {
        apply_path = kustomize::render_overlay_copy(options.workspace_root, &resolved_path)?;
        if let Some(image_map) = &options.image_map {
            println!(
                "==> Rendering image overrides into {}",
                apply_path.display()
            );
            kustomize::set_overlay_images(&apply_path, image_map)?;
        }
        if options.expose_direct_ingress {
            println!("==> Enabling direct-ingress exposure (ws ingress + postgres debug NodePort)");
            kustomize::add_component(
                options.workspace_root,
                &apply_path,
                "components/direct-ingress",
            )?;
        }
    } else if options.expose_direct_ingress && !is_overlay {
        eprintln!(
            "warning: --expose-direct-ingress only applies to kustomize overlays; ignoring for a raw manifest path."
        );
    }

    let ctx_args = context_args(options.kube_context);
    let verb = if options.delete { "delete" } else { "apply" };
    let flag = if is_overlay { "-k" } else { "-f" };
    let apply_path_str = apply_path.display().to_string();

    if options.command_center_only {
        let names = ["runinator-command-center"];
        let rendered = render_manifest(options.workspace_root, &ctx_args, &apply_path, is_overlay)?;
        let docs = yaml_docs::parse_documents(&rendered)?;
        let filtered = yaml_docs::select_by_names(&docs, &names);
        anyhow::ensure!(
            !filtered.is_empty(),
            "no command-center web resources were found in {}",
            apply_path.display()
        );
        let stdin = yaml_docs::serialize_documents(&filtered)?;

        println!("==> kubectl {} {verb} -f -", ctx_args.join(" "));
        let mut apply_args = kubectl_args(&ctx_args, &[verb]);
        if options.delete {
            apply_args.push("--ignore-not-found=true");
        }
        apply_args.push("-f");
        apply_args.push("-");
        exec::run_with_stdin("kubectl", &apply_args, options.workspace_root, &stdin)?;
        if options.delete {
            return Ok(());
        }

        let rollout_target =
            yaml_docs::rollout_target(&docs, "runinator-command-center", "Deployment");
        run_rollout_checks(options.workspace_root, &ctx_args, &[rollout_target]);
        return Ok(());
    }

    println!(
        "==> kubectl {} {verb} {flag} {apply_path_str}",
        ctx_args.join(" ")
    );
    for stale_resource in STALE_RESOURCES {
        exec::warn_on_err(
            &format!("stale-resource cleanup skipped or failed for '{stale_resource}'"),
            || {
                let args = kubectl_args(
                    &ctx_args,
                    &[
                        "delete",
                        stale_resource,
                        "--namespace",
                        NAMESPACE,
                        "--ignore-not-found=true",
                    ],
                );
                exec::run("kubectl", &args, options.workspace_root)
            },
        );
    }

    let mut skip_pg = false;
    let mut skip_mq = false;
    if !options.delete && !options.recreate_infra && is_overlay {
        skip_pg = resource_exists(
            options.workspace_root,
            &ctx_args,
            "statefulset",
            "runinator-postgres",
        );
        skip_mq = resource_exists(
            options.workspace_root,
            &ctx_args,
            "statefulset",
            "runinator-rabbitmq",
        );
        if skip_pg {
            println!(
                "==> Preserving existing statefulset/runinator-postgres (pass --recreate-infra to override)"
            );
        }
        if skip_mq {
            println!(
                "==> Preserving existing statefulset/runinator-rabbitmq (pass --recreate-infra to override)"
            );
        }
    }

    if options.delete {
        let args = kubectl_args(
            &ctx_args,
            &[verb, flag, &apply_path_str, "--ignore-not-found=true"],
        );
        exec::run("kubectl", &args, options.workspace_root)?;
        return Ok(());
    } else if !skip_pg && !skip_mq {
        let args = kubectl_args(&ctx_args, &[verb, flag, &apply_path_str]);
        exec::run("kubectl", &args, options.workspace_root)?;
    } else {
        let rendered = kustomize_render(options.workspace_root, &ctx_args, &apply_path)?;
        let docs = yaml_docs::parse_documents(&rendered)?;
        let mut skip_names = Vec::new();
        if skip_pg {
            skip_names.push("runinator-postgres");
        }
        if skip_mq {
            skip_names.push("runinator-rabbitmq");
        }
        let filtered = yaml_docs::filter_out_statefulsets(&docs, &skip_names);
        let stdin = yaml_docs::serialize_documents(&filtered)?;
        let args = kubectl_args(&ctx_args, &["apply", "-f", "-"]);
        exec::run_with_stdin("kubectl", &args, options.workspace_root, &stdin)?;
    }

    let rendered_manifest =
        render_manifest(options.workspace_root, &ctx_args, &apply_path, is_overlay)?;
    let docs = yaml_docs::parse_documents(&rendered_manifest)?;

    remove_superseded_workload_controllers(options.workspace_root, &ctx_args, &docs);

    let mut rollout_targets = Vec::new();
    if !skip_pg {
        rollout_targets.push(yaml_docs::rollout_target(
            &docs,
            "runinator-postgres",
            "StatefulSet",
        ));
    }
    if !skip_mq {
        rollout_targets.push(yaml_docs::rollout_target(
            &docs,
            "runinator-rabbitmq",
            "StatefulSet",
        ));
    }
    for (name, fallback_kind) in [
        ("runinator-ws", "Deployment"),
        ("runinator-engine-worker", "Deployment"),
        ("runinator-archiver", "Deployment"),
        ("runinator-waker", "Deployment"),
        ("runinator-worker", "StatefulSet"),
        ("runinator-command-center", "Deployment"),
    ] {
        rollout_targets.push(yaml_docs::rollout_target(&docs, name, fallback_kind));
    }

    run_rollout_checks(options.workspace_root, &ctx_args, &rollout_targets);

    Ok(())
}

/// Apply only the resources owned by the Grafana dashboard. This deliberately filters the
/// rendered overlay instead of applying the observability component directly, so the command
/// works with the same local/prod overlay and never rolls the collector, Prometheus, or Runinator
/// runtime workloads.
pub fn redeploy_grafana(options: GrafanaRedeployOptions) -> Result<()> {
    exec::require_tool("kubectl")?;

    let resolved_path = if options.manifest_path.is_absolute() {
        options.manifest_path.to_path_buf()
    } else {
        options.workspace_root.join(options.manifest_path)
    };
    anyhow::ensure!(
        resolved_path.is_dir(),
        "Grafana redeploy requires a kustomize overlay directory, got {}",
        resolved_path.display()
    );

    let ctx_args = context_args(options.kube_context);
    let overlay_path = resolved_path.display().to_string();
    let rendered = kustomize_render(options.workspace_root, &ctx_args, &resolved_path)?;
    let docs = yaml_docs::parse_documents(&rendered)?;
    let names = [
        "runinator-grafana-datasources",
        "runinator-grafana-dashboards-provider",
        "runinator-grafana-dashboard-overview",
        "runinator-grafana",
    ];
    let filtered = yaml_docs::select_by_names(&docs, &names);
    anyhow::ensure!(
        !filtered.is_empty(),
        "no Grafana resources were found in kustomize overlay {overlay_path}"
    );
    let stdin = yaml_docs::serialize_documents(&filtered)?;
    let apply_args = kubectl_args(&ctx_args, &["apply", "-f", "-"]);
    println!("==> Applying Grafana resources from {overlay_path}");
    exec::run_with_stdin("kubectl", &apply_args, options.workspace_root, &stdin)?;

    let rollout = yaml_docs::rollout_target(&filtered, "runinator-grafana", "Deployment");
    run_rollout_checks(options.workspace_root, &ctx_args, &[rollout]);
    Ok(())
}

/// Apply only PostgreSQL's Service and StatefulSet from a rendered overlay. Re-applying the
/// StatefulSet updates its pod template without deleting its PVC. `from_scratch` instead scales
/// PostgreSQL down and deletes its sole generated data claim before recreating the StatefulSet.
/// It then restarts only the web service to run its database bootstrap, so the newly empty database
/// becomes usable without applying the rest of the stack.
pub fn redeploy_database(options: DatabaseRedeployOptions) -> Result<()> {
    exec::require_tool("kubectl")?;

    let resolved_path = if options.manifest_path.is_absolute() {
        options.manifest_path.to_path_buf()
    } else {
        options.workspace_root.join(options.manifest_path)
    };
    anyhow::ensure!(
        resolved_path.is_dir(),
        "Database redeploy requires a kustomize overlay directory, got {}",
        resolved_path.display()
    );

    let ctx_args = context_args(options.kube_context);
    let overlay_path = resolved_path.display().to_string();
    let rendered = kustomize_render(options.workspace_root, &ctx_args, &resolved_path)?;
    let docs = yaml_docs::parse_documents(&rendered)?;
    let filtered = yaml_docs::select_by_names(&docs, &["runinator-postgres"]);
    anyhow::ensure!(
        filtered
            .iter()
            .any(|doc| yaml_docs::doc_kind(doc) == Some("StatefulSet")),
        "no PostgreSQL StatefulSet was found in kustomize overlay {overlay_path}"
    );

    if options.from_scratch {
        reset_postgres_data(options.workspace_root, &ctx_args, &filtered)?;
    }

    let stdin = yaml_docs::serialize_documents(&filtered)?;
    let apply_args = kubectl_args(&ctx_args, &["apply", "-f", "-"]);
    println!("==> Applying PostgreSQL resources from {overlay_path}");
    exec::run_with_stdin("kubectl", &apply_args, options.workspace_root, &stdin)?;

    let rollout = yaml_docs::rollout_target(&filtered, "runinator-postgres", "StatefulSet");
    run_rollout_checks(options.workspace_root, &ctx_args, &[rollout]);

    if options.from_scratch {
        bootstrap_fresh_database(options.workspace_root, &ctx_args, &docs)?;
    }

    Ok(())
}

/// Kubernetes names StatefulSet claims as `<template-name>-<statefulset-name>-<ordinal>`. The
/// bundled Postgres StatefulSet has one replica and therefore always uses ordinal zero. Refuse to
/// guess if an overlay grows more than one claim template: deleting the wrong PVC is worse than
/// making the operator select a purpose-built reset command.
pub(super) fn postgres_data_claim_name(docs: &[Value]) -> Result<String> {
    let statefulset = docs
        .iter()
        .find(|doc| {
            yaml_docs::doc_kind(doc) == Some("StatefulSet")
                && yaml_docs::doc_name(doc) == Some("runinator-postgres")
        })
        .ok_or_else(|| {
            anyhow::anyhow!("rendered manifest does not contain statefulset/runinator-postgres")
        })?;
    let statefulset_name = yaml_docs::doc_name(statefulset)
        .expect("PostgreSQL StatefulSet was selected by its metadata.name");
    let claims = statefulset
        .get("spec")
        .and_then(|spec| spec.get("volumeClaimTemplates"))
        .and_then(Value::as_sequence)
        .ok_or_else(|| {
            anyhow::anyhow!("statefulset/{statefulset_name} has no volumeClaimTemplates")
        })?;
    anyhow::ensure!(
        claims.len() == 1,
        "refusing to reset statefulset/{statefulset_name}: expected exactly one volumeClaimTemplate, found {}",
        claims.len()
    );
    let template_name = claims[0]
        .get("metadata")
        .and_then(|metadata| metadata.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("statefulset/{statefulset_name} has an unnamed volumeClaimTemplate")
        })?;
    Ok(format!("{template_name}-{statefulset_name}-0"))
}

fn reset_postgres_data(workspace_root: &Path, ctx_args: &[String], docs: &[Value]) -> Result<()> {
    let claim_name = postgres_data_claim_name(docs)?;

    println!("==> Scaling down PostgreSQL before deleting data claim {claim_name}");
    let scale_args = kubectl_args(
        ctx_args,
        &[
            "scale",
            "statefulset/runinator-postgres",
            "--namespace",
            NAMESPACE,
            "--replicas=0",
        ],
    );
    exec::run("kubectl", &scale_args, workspace_root)?;

    // `--wait` ensures that the Postgres pod has released the claim before the command returns.
    // This makes a PVC backed by a ReadWriteOnce volume safe to replace without separately polling
    // a pod that may already have disappeared by the time `kubectl wait` is invoked.
    let delete_args = kubectl_args(
        ctx_args,
        &[
            "delete",
            "pvc",
            &claim_name,
            "--namespace",
            NAMESPACE,
            "--ignore-not-found=true",
            "--wait=true",
            "--timeout=120s",
        ],
    );
    exec::run("kubectl", &delete_args, workspace_root)
}

fn bootstrap_fresh_database(
    workspace_root: &Path,
    ctx_args: &[String],
    docs: &[Value],
) -> Result<()> {
    println!("==> Restarting the web service to bootstrap the fresh database");
    let restart_args = kubectl_args(
        ctx_args,
        &[
            "rollout",
            "restart",
            "deployment/runinator-ws",
            "--namespace",
            NAMESPACE,
        ],
    );
    exec::run("kubectl", &restart_args, workspace_root)?;
    run_rollout_checks(
        workspace_root,
        ctx_args,
        &[yaml_docs::rollout_target(
            docs,
            "runinator-ws",
            "Deployment",
        )],
    );

    Ok(())
}

fn run_rollout_checks(workspace_root: &Path, ctx_args: &[String], targets: &[String]) {
    for target in targets {
        exec::warn_on_err(
            &format!("rollout status check failed for '{target}'"),
            || {
                let args = kubectl_args(
                    ctx_args,
                    &[
                        "rollout",
                        "status",
                        target,
                        "--namespace",
                        NAMESPACE,
                        "--timeout",
                        "120s",
                    ],
                );
                exec::run("kubectl", &args, workspace_root)
            },
        );
    }
}

/// deletes whichever stale controller kind (`Deployment` vs `StatefulSet`) the desired manifest no
/// longer uses for worker/waker, so a kind change (e.g. worker moving from Deployment to
/// StatefulSet) doesn't leave the old controller running alongside the new one.
fn remove_superseded_workload_controllers(
    workspace_root: &Path,
    ctx_args: &[String],
    docs: &[Value],
) {
    for name in ["runinator-worker", "runinator-waker"] {
        let Some(desired_kind) = yaml_docs::workload_kind(docs, name) else {
            eprintln!(
                "warning: could not determine desired workload kind for {name}; skipping stale workload cleanup."
            );
            continue;
        };

        let stale_kind = if desired_kind == "Deployment" {
            "statefulset"
        } else {
            "deployment"
        };
        let stale_resource = format!("{stale_kind}/{name}");
        println!(
            "==> Removing superseded {stale_resource} for desired {}/{name}",
            desired_kind.to_lowercase()
        );
        let args = kubectl_args(
            ctx_args,
            &[
                "delete",
                &stale_resource,
                "--namespace",
                NAMESPACE,
                "--ignore-not-found=true",
                "--wait=true",
                "--timeout=120s",
            ],
        );
        exec::warn_on_err(&format!("failed removing {stale_resource}"), || {
            exec::run("kubectl", &args, workspace_root)
        });
    }
}
