use std::collections::BTreeMap;

use async_trait::async_trait;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{DeleteParams, Patch, PatchParams};
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
        _spec: &NodeSpec,
    ) -> Result<ProvisionedGroup, SendableError> {
        let name = self.deployment_name(kind)?.to_string();
        let api = self.deployments_api().await?;
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
