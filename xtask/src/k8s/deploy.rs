//! orchestrates a kustomize-based apply/delete against a running cluster, mirroring build.ps1's
//! `Deploy-KubernetesStack`: renders image overrides into a disposable overlay copy, preserves
//! already-running postgres/rabbitmq state unless asked to recreate it, cleans up resources that
//! were superseded by earlier renames, and waits for rollouts + the pack-import job.

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
    pub pack_import_timeout_secs: u32,
    pub image_map: Option<HashMap<String, String>>,
    pub delete: bool,
    pub command_center_only: bool,
    pub recreate_infra: bool,
    pub expose_direct_ingress: bool,
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
            &format!("pack-import cleanup skipped or failed for '{stale_resource}'"),
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
        ("runinator-background", "Deployment"),
        ("runinator-archiver", "Deployment"),
        ("runinator-waker", "Deployment"),
        ("runinator-worker", "StatefulSet"),
        ("runinator-command-center", "Deployment"),
    ] {
        rollout_targets.push(yaml_docs::rollout_target(&docs, name, fallback_kind));
    }

    run_rollout_checks(options.workspace_root, &ctx_args, &rollout_targets);

    exec::warn_on_err("pack-import job did not complete within timeout", || {
        let timeout = format!("{}s", options.pack_import_timeout_secs);
        let args = kubectl_args(
            &ctx_args,
            &[
                "wait",
                "--for=condition=complete",
                "job/runinator-pack-import",
                "--namespace",
                NAMESPACE,
                "--timeout",
                &timeout,
            ],
        );
        exec::run("kubectl", &args, options.workspace_root)
    });

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
