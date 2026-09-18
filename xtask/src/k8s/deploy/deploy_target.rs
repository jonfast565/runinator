#[allow(unused_imports)]
use super::*;

/// one independently deployable runinator workload: the container images it runs, the rendered
/// resources it owns, and the controller a rollout waits on. selecting targets lets a deploy
/// rebuild and roll only the components a change touched instead of the whole stack.
#[derive(Debug)]
pub struct DeployTarget {
    /// value accepted by `--service`.
    pub key: &'static str,
    /// every image referenced by the workload's containers, including sidecars and init
    /// containers. all of them must be rebuilt, or the workload would roll with a stale tag for
    /// the containers left out of the image map.
    pub images: &'static [&'static str],
    /// `metadata.name` of each rendered resource the workload owns, across every kind.
    pub resources: &'static [&'static str],
    /// the workload controller to wait on, and its kind when the manifest does not say.
    pub workload: &'static str,
    pub workload_kind: &'static str,
}

const TARGETS: &[DeployTarget] = &[
    DeployTarget {
        key: "ws",
        // the ws pod runs the bootstrap init container and the adapter-host sidecar alongside it.
        images: &[
            "runinator-ws",
            "runinator-adapter-host",
            "runinator-bootstrap",
        ],
        resources: &["runinator-ws", "runinator-ws-provisioner"],
        workload: "runinator-ws",
        workload_kind: "Deployment",
    },
    DeployTarget {
        key: "engine-worker",
        images: &["runinator-engine-worker", "runinator-adapter-host"],
        resources: &["runinator-engine-worker"],
        workload: "runinator-engine-worker",
        workload_kind: "Deployment",
    },
    DeployTarget {
        key: "worker",
        images: &["runinator-worker"],
        resources: &["runinator-worker"],
        workload: "runinator-worker",
        workload_kind: "StatefulSet",
    },
    DeployTarget {
        key: "waker",
        images: &["runinator-waker"],
        resources: &["runinator-waker"],
        workload: "runinator-waker",
        workload_kind: "Deployment",
    },
    DeployTarget {
        key: "archiver",
        images: &["runinator-archiver"],
        resources: &["runinator-archiver", "runinator-archive-data"],
        workload: "runinator-archiver",
        workload_kind: "Deployment",
    },
    DeployTarget {
        key: "blob",
        images: &["runinator-blob"],
        resources: &["runinator-blob", "runinator-blob-data"],
        workload: "runinator-blob",
        workload_kind: "Deployment",
    },
    DeployTarget {
        key: "command-center",
        images: &["runinator-command-center"],
        resources: &["runinator-command-center"],
        workload: "runinator-command-center",
        workload_kind: "Deployment",
    },
];

impl DeployTarget {
    /// the `--service` values, for help text and error messages.
    pub fn keys() -> Vec<&'static str> {
        TARGETS.iter().map(|target| target.key).collect()
    }

    /// resolves `--service` values, rejecting an unknown name rather than silently deploying less
    /// than the operator asked for.
    pub fn resolve(keys: &[String]) -> Result<Vec<&'static DeployTarget>> {
        let mut resolved: Vec<&'static DeployTarget> = Vec::new();
        for key in keys {
            let key = key.trim();
            let Some(target) = TARGETS.iter().find(|target| target.key == key) else {
                anyhow::bail!(
                    "unknown service '{key}'; expected one of: {}",
                    Self::keys().join(", ")
                );
            };
            if !resolved.iter().any(|existing| existing.key == target.key) {
                resolved.push(target);
            }
        }
        Ok(resolved)
    }

    /// the images the selected targets need built, deduplicated across shared sidecars.
    pub fn images_for(targets: &[&'static DeployTarget]) -> Vec<&'static str> {
        let mut images: Vec<&'static str> = Vec::new();
        for image in targets.iter().flat_map(|target| target.images.iter()) {
            if !images.contains(image) {
                images.push(image);
            }
        }
        images
    }

    /// the rendered resource names the selected targets own.
    pub fn resources_for(targets: &[&'static DeployTarget]) -> Vec<&'static str> {
        let mut resources: Vec<&'static str> = Vec::new();
        for resource in targets.iter().flat_map(|target| target.resources.iter()) {
            if !resources.contains(resource) {
                resources.push(resource);
            }
        }
        resources
    }
}
