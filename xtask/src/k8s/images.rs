//! builds (and optionally pushes) the runinator container images, mirroring build.ps1's
//! `Build-ContainerImages`. all rust services share `deploy/Dockerfile`, selected via `--target`;
//! BuildKit caches the shared builder stage so the cargo compile runs once for the whole set.

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
/// as-is, otherwise a fresh `kube-<timestamp>` tag is generated so every k8s deploy gets a distinct,
/// traceable image without the caller needing to think about tagging.
pub fn versioned_image_tag(requested_tag: &str) -> String {
    if !requested_tag.trim().is_empty() && requested_tag != "local" {
        return requested_tag.to_string();
    }
    format!("kube-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"))
}

/// builds (and optionally pushes) the selected images, returning a map of image name -> tagged
/// reference for the ones that were built.
pub fn build_container_images(
    workspace_root: &Path,
    repository: Option<&str>,
    tag: &str,
    include_names: Option<&[&str]>,
    exclude_names: Option<&[&str]>,
    push_images: bool,
) -> Result<HashMap<String, String>> {
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

    let mut built = HashMap::new();
    for image in images {
        let tagged_name = image_tag(image.name, repository, tag);
        println!("\n==> Building image {tagged_name}");

        let mut args: Vec<String> = vec![
            "build".to_string(),
            "--file".to_string(),
            image.dockerfile.to_string(),
        ];
        if let Some(target) = image.target {
            args.push("--target".to_string());
            args.push(target.to_string());
        }
        args.push("--tag".to_string());
        args.push(tagged_name.clone());
        args.push(image.context.to_string());

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        exec::run("docker", &arg_refs, workspace_root)?;
        built.insert(image.name.to_string(), tagged_name.clone());

        if push_images {
            println!("\n==> Pushing image {tagged_name}");
            exec::run("docker", &["push", &tagged_name], workspace_root)?;
        }
    }

    Ok(built)
}
