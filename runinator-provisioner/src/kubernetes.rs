use std::collections::BTreeMap;

use async_trait::async_trait;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{EnvVar, Pod};
use kube::api::{DeleteParams, Patch, PatchParams, PostParams};
use kube::{Api, Client};
use runinator_models::errors::SendableError;
use runinator_models::provisioning::{NodeSpec, ProvisionBackend, ProvisionedGroup};
use runinator_models::replicas::ReplicaKind;

use crate::errors::{KUBERNETES_API, KUBERNETES_INIT, UNSUPPORTED_KIND};
use crate::traits::Provisioner;

/// provisions worker/waker nodes by scaling their Kubernetes Deployments.
pub struct KubernetesProvisioner {
    namespace: String,
    // node kind -> deployment name.
    deployments: BTreeMap<&'static str, String>,
}

impl KubernetesProvisioner {
    pub fn new(namespace: String) -> Self {
        Self {
            namespace,
            deployments: BTreeMap::new(),
        }
    }

    /// register the Deployment that backs a node kind. only mapped kinds are manageable.
    pub fn with_deployment(mut self, kind: ReplicaKind, name: impl Into<String>) -> Self {
        self.deployments.insert(kind.as_str(), name.into());
        self
    }

    fn deployment_name(&self, kind: ReplicaKind) -> Result<&str, SendableError> {
        self.deployments
            .get(kind.as_str())
            .map(String::as_str)
            .ok_or_else(|| UNSUPPORTED_KIND.error(kind.as_str()))
    }

    async fn client(&self) -> Result<Client, SendableError> {
        Client::try_default()
            .await
            .map_err(|err| KUBERNETES_INIT.error(err))
    }

    async fn deployments_api(&self) -> Result<Api<Deployment>, SendableError> {
        Ok(Api::namespaced(self.client().await?, &self.namespace))
    }

    fn group(
        &self,
        kind: ReplicaKind,
        name: &str,
        desired: u32,
        available: u32,
    ) -> ProvisionedGroup {
        ProvisionedGroup {
            backend: ProvisionBackend::Kubernetes,
            kind,
            name: name.to_string(),
            desired,
            available,
            manageable: true,
        }
    }
}

// build a per-group Deployment by cloning a base one: rename it, reset server-managed identity, set
// the desired replica count, merge the group's routing labels into metadata/selector/pod labels, and
// export them as `RUNINATOR_WORKER_LABELS` so spawned workers advertise the group's affinity labels.
pub(crate) fn clone_group_deployment(
    mut base: Deployment,
    name: &str,
    desired: u32,
    spec: &NodeSpec,
) -> Deployment {
    base.status = None;
    let meta = &mut base.metadata;
    meta.name = Some(name.to_string());
    // drop server-assigned identity so the object is accepted as a fresh create.
    meta.resource_version = None;
    meta.uid = None;
    meta.creation_timestamp = None;
    meta.generation = None;
    meta.managed_fields = None;
    meta.self_link = None;
    merge_labels(meta.labels.get_or_insert_with(BTreeMap::new), &spec.labels);

    if let Some(dspec) = base.spec.as_mut() {
        dspec.replicas = Some(desired as i32);
        // the selector must match the pod template labels, so merge the group labels into both.
        merge_labels(
            dspec
                .selector
                .match_labels
                .get_or_insert_with(BTreeMap::new),
            &spec.labels,
        );
        let template_meta = dspec.template.metadata.get_or_insert_with(Default::default);
        merge_labels(
            template_meta.labels.get_or_insert_with(BTreeMap::new),
            &spec.labels,
        );
        if let Some(pod_spec) = dspec.template.spec.as_mut() {
            let labels_env = spec
                .labels
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",");
            for container in &mut pod_spec.containers {
                set_env(
                    container.env.get_or_insert_with(Vec::new),
                    "RUNINATOR_WORKER_LABELS",
                    &labels_env,
                );
            }
        }
    }
    base
}

// merge `src` label pairs into `dst`, overwriting on key collision.
fn merge_labels(dst: &mut BTreeMap<String, String>, src: &BTreeMap<String, String>) {
    for (key, value) in src {
        dst.insert(key.clone(), value.clone());
    }
}

// set (replace) an environment variable by name in a container env list.
fn set_env(env: &mut Vec<EnvVar>, name: &str, value: &str) {
    env.retain(|var| var.name != name);
    env.push(EnvVar {
        name: name.to_string(),
        value: Some(value.to_string()),
        value_from: None,
    });
}

#[async_trait]
impl Provisioner for KubernetesProvisioner {
    fn backend(&self) -> ProvisionBackend {
        ProvisionBackend::Kubernetes
    }

    fn supported_kinds(&self) -> Vec<ReplicaKind> {
        let mut kinds = Vec::new();
        for kind in [ReplicaKind::Worker, ReplicaKind::Waker] {
            if self.deployments.contains_key(kind.as_str()) {
                kinds.push(kind);
            }
        }
        kinds
    }

    async fn available(&self) -> bool {
        self.client().await.is_ok()
    }

    async fn list(&self) -> Result<Vec<ProvisionedGroup>, SendableError> {
        let api = self.deployments_api().await?;
        let mut groups = Vec::new();
        for kind in self.supported_kinds() {
            let name = self.deployment_name(kind)?.to_string();
            let deployment = api
                .get(&name)
                .await
                .map_err(|err| KUBERNETES_API.error(err))?;
            let desired = deployment
                .spec
                .as_ref()
                .and_then(|s| s.replicas)
                .unwrap_or(0)
                .max(0) as u32;
            let available = deployment
                .status
                .as_ref()
                .and_then(|s| s.available_replicas)
                .unwrap_or(0)
                .max(0) as u32;
            groups.push(self.group(kind, &name, desired, available));
        }
        Ok(groups)
    }

    async fn scale(
        &self,
        kind: ReplicaKind,
        desired: u32,
        spec: &NodeSpec,
    ) -> Result<ProvisionedGroup, SendableError> {
        // a per-group scale (e.g. a per-org pool) targets a deployment named by the group; otherwise
        // the kind's default deployment.
        let name = match &spec.group {
            Some(group) => group.clone(),
            None => self.deployment_name(kind)?.to_string(),
        };
        let api = self.deployments_api().await?;

        // a per-group deployment that does not exist yet is created by cloning the kind's base
        // deployment, renamed and labeled for the group, so per-org pools are self-provisioning.
        if spec.group.is_some()
            && api
                .get_opt(&name)
                .await
                .map_err(|err| KUBERNETES_API.error(err))?
                .is_none()
        {
            let base_name = self.deployment_name(kind)?.to_string();
            let base = api
                .get(&base_name)
                .await
                .map_err(|err| KUBERNETES_API.error(err))?;
            let deployment = clone_group_deployment(base, &name, desired, spec);
            api.create(&PostParams::default(), &deployment)
                .await
                .map_err(|err| KUBERNETES_API.error(err))?;
            return Ok(self.group(kind, &name, desired, 0));
        }

        let patch = serde_json::json!({ "spec": { "replicas": desired } });
        api.patch_scale(&name, &PatchParams::default(), &Patch::Merge(&patch))
            .await
            .map_err(|err| KUBERNETES_API.error(err))?;

        let deployment = api
            .get(&name)
            .await
            .map_err(|err| KUBERNETES_API.error(err))?;
        let available = deployment
            .status
            .as_ref()
            .and_then(|s| s.available_replicas)
            .unwrap_or(0)
            .max(0) as u32;
        Ok(self.group(kind, &name, desired, available))
    }

    async fn stop(&self, node_id: &str) -> Result<(), SendableError> {
        // node_id is a pod name; deleting it lets the Deployment reschedule (best-effort drain).
        let api: Api<Pod> = Api::namespaced(self.client().await?, &self.namespace);
        api.delete(node_id, &DeleteParams::default())
            .await
            .map_err(|err| KUBERNETES_API.error(err))?;
        Ok(())
    }
}
