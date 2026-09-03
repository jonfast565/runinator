//! builds (and optionally pushes) the runinator container images. all rust services share
//! `deploy/Dockerfile`, selected via `--target`; BuildKit caches the shared builder stage so the
//! cargo compile runs once for the whole set, and the builder's cargo cache mounts make each
//! subsequent compile incremental rather than from scratch.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, bail};

use crate::exec;

struct ImageSpec {
    name: &'static str,
    dockerfile: &'static str,
    target: Option<&'static str>,
    context: &'static str,
}

const IMAGES: &[ImageSpec] = &[
    ImageSpec {
        name: "runinator-waker",
        dockerfile: "deploy/Dockerfile",
        target: Some("waker"),
        context: ".",
    },
    ImageSpec {
        name: "runinator-worker",
        dockerfile: "deploy/Dockerfile",
        target: Some("worker"),
        context: ".",
    },
    ImageSpec {
        name: "runinator-archiver",
        dockerfile: "deploy/Dockerfile",
        target: Some("archiver"),
        context: ".",
    },
    ImageSpec {
        name: "runinator-ctl",
        dockerfile: "deploy/Dockerfile",
        target: Some("ctl"),
        context: ".",
    },
    ImageSpec {
        name: "runinator-ws",
        dockerfile: "deploy/Dockerfile",
        target: Some("ws"),
        context: ".",
    },
    ImageSpec {
        name: "runinator-engine-worker",
        dockerfile: "deploy/Dockerfile",
        target: Some("engine-worker"),
        context: ".",
    },
    ImageSpec {
        name: "runinator-blob",
        dockerfile: "deploy/Dockerfile",
        target: Some("blob"),
        context: ".",
    },
    ImageSpec {
        name: "runinator-adapter-host",
        dockerfile: "deploy/Dockerfile",
        target: Some("adapter-host"),
        context: ".",
    },
    ImageSpec {
        name: "runinator-bootstrap",
        dockerfile: "deploy/Dockerfile",
        target: Some("bootstrap"),
        context: ".",
    },
    ImageSpec {
        name: "runinator-command-center",
        dockerfile: "runinator-command-center/Dockerfile",
        target: None,
        context: "runinator-command-center",
    },
];

pub fn image_tag(name: &str, repository: Option<&str>, tag: &str) -> String {
    match repository {
        Some(repository) if !repository.trim().is_empty() => format!("{repository}/{name}:{tag}"),
        _ => format!("{name}:{tag}"),
    }
}

/// resolves the requested `--image-tag` to a concrete value: an explicit non-`local` tag is used
/// as-is, otherwise the workspace version and a timestamp identify the deployment build.
pub fn versioned_image_tag(requested_tag: &str) -> String {
    if !requested_tag.trim().is_empty() && requested_tag != "local" {
        return requested_tag.to_string();
    }
    format!(
        "{}-kube-{}",
        env!("CARGO_PKG_VERSION"),
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    )
}

/// the short commit the build came from, or empty outside a git checkout.
fn current_commit(workspace_root: &Path) -> String {
    exec::capture_allow_failure("git", &["rev-parse", "--short", "HEAD"], workspace_root)
        .trim()
        .to_string()
}

/// builds (and optionally pushes) the selected images, returning a map of image name -> tagged
/// reference for the ones that were built.
pub struct ContainerImageBuild<'a> {
    pub repository: Option<&'a str>,
    pub tag: &'a str,
    pub include_names: Option<&'a [&'a str]>,
    pub exclude_names: Option<&'a [&'a str]>,
    pub push_images: bool,
    pub database_backend: &'a str,
    pub broker_backend: &'a str,
}

pub fn build_container_images(
    workspace_root: &Path,
    request: ContainerImageBuild<'_>,
) -> Result<HashMap<String, String>> {
    let ContainerImageBuild {
        repository,
        tag,
        include_names,
        exclude_names,
        push_images,
        database_backend,
        broker_backend,
    } = request;
    exec::require_tool("docker")?;

    let mut images: Vec<&ImageSpec> = IMAGES.iter().collect();
    if let Some(include) = include_names {
        images.retain(|image| include.contains(&image.name));
    }
    if let Some(exclude) = exclude_names {
        images.retain(|image| !exclude.contains(&image.name));
    }
    if images.is_empty() {
        bail!("no container images were selected for build");
    }

    // The command center stamps this into its UI version readout; its build context has no .git.
    let build_id = current_commit(workspace_root);

    let mut built = HashMap::new();
    for image in images {
        let tagged_name = image_tag(image.name, repository, tag);
        println!("\n==> Building image {tagged_name}");

        let mut args: Vec<String> = vec![
            "build".to_string(),
            "--file".to_string(),
            image.dockerfile.to_string(),
            "--build-arg".to_string(),
            format!("RUNINATOR_VERSION={}", env!("CARGO_PKG_VERSION")),
        ];
        if image.name == "runinator-command-center" && !build_id.is_empty() {
            args.push("--build-arg".to_string());
            args.push(format!("RUNINATOR_BUILD_ID={build_id}"));
        }
        if let Some(target) = image.target {
            args.push("--target".to_string());
            args.push(target.to_string());
        }
        if image.dockerfile == "deploy/Dockerfile" {
            args.push("--build-arg".to_string());
            args.push(format!("RUNINATOR_K8S_DATABASE_BACKEND={database_backend}"));
            args.push("--build-arg".to_string());
            args.push(format!("RUNINATOR_K8S_BROKER_BACKEND={broker_backend}"));
        }
        args.push("--tag".to_string());
        args.push(tagged_name.clone());
        args.push(image.context.to_string());

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        // deploy/Dockerfile's cargo cache mounts are BuildKit-only syntax; forcing it on keeps a
        // daemon still defaulting to the legacy builder from failing on `--mount=type=cache`.
        exec::run_with_env(
            "docker",
            &arg_refs,
            workspace_root,
            &[("DOCKER_BUILDKIT", "1")],
        )?;
        built.insert(image.name.to_string(), tagged_name.clone());

        if push_images {
            println!("\n==> Pushing image {tagged_name}");
            exec::run("docker", &["push", &tagged_name], workspace_root)?;
        }
    }

    Ok(built)
}
