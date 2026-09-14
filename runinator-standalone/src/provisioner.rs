use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use runinator_models::{
    errors::SendableError,
    provisioning::{NodeSpec, ProvisionBackend, ProvisionedGroup},
    replicas::ReplicaKind,
};
use runinator_provisioner::Provisioner;
use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
};
use uuid::Uuid;

use crate::state::StateTracker;

#[async_trait]
pub trait RuntimeFactory: Send + Sync {
    async fn spawn(
        &self,
        kind: ReplicaKind,
        node_id: String,
        spec: NodeSpec,
        shutdown: Arc<Notify>,
    ) -> Result<JoinHandle<()>, SendableError>;
}

struct Node {
    kind: ReplicaKind,
    group: String,
    spec: NodeSpec,
    shutdown: Arc<Notify>,
    task: JoinHandle<()>,
}

pub struct StandaloneProvisioner {
    factory: Arc<dyn RuntimeFactory>,
    nodes: Mutex<BTreeMap<String, Node>>,
    tracker: StateTracker,
}

impl StandaloneProvisioner {
    pub fn new(factory: Arc<dyn RuntimeFactory>, tracker: StateTracker) -> Self {
        Self {
            factory,
            nodes: Mutex::new(BTreeMap::new()),
            tracker,
        }
    }

    fn manageable(kind: ReplicaKind) -> bool {
        matches!(
            kind,
            ReplicaKind::Worker | ReplicaKind::Waker | ReplicaKind::Background
        )
    }

    async fn add_node(
        &self,
        kind: ReplicaKind,
        group: String,
        spec: NodeSpec,
    ) -> Result<(), SendableError> {
        let id = format!("standalone-{}-{}", kind.as_str(), Uuid::now_v7());
        self.spawn_node(id, kind, group, spec).await
    }

    async fn spawn_node(
        &self,
        id: String,
        kind: ReplicaKind,
        group: String,
        spec: NodeSpec,
    ) -> Result<(), SendableError> {
        let shutdown = Arc::new(Notify::new());
        let task = self
            .factory
            .spawn(kind, id.clone(), spec.clone(), shutdown.clone())
            .await?;
        self.nodes.lock().await.insert(
            id,
            Node {
                kind,
                group,
                spec,
                shutdown,
                task,
            },
        );
        Ok(())
    }

    async fn remove_node(&self, id: &str) -> Result<(), SendableError> {
        let node = self.nodes.lock().await.remove(id);
        let Some(node) = node else {
            return Err(format!("standalone node '{id}' was not found").into());
        };
        node.shutdown.notify_one();
        node.shutdown.notify_waiters();
        match tokio::time::timeout(std::time::Duration::from_secs(15), node.task).await {
            Ok(_) => {}
            Err(_) => self.tracker.failed(id, "graceful shutdown timed out"),
        }
        self.tracker.stopped(id);
        Ok(())
    }

    pub async fn shutdown_all(&self) {
        let ids = self.nodes.lock().await.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let _ = self.remove_node(&id).await;
        }
    }

    pub async fn supervise(self: Arc<Self>, shutdown: Arc<Notify>) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = shutdown.notified() => return,
                _ = interval.tick() => {}
            }
            let finished = {
                let nodes = self.nodes.lock().await;
                nodes
                    .iter()
                    .find(|(_, node)| node.task.is_finished())
                    .map(|(id, _)| id.clone())
            };
            let Some(id) = finished else {
                continue;
            };
            let node = self.nodes.lock().await.remove(&id);
            let Some(node) = node else {
                continue;
            };
            let _ = node.task.await;
            self.tracker.restarting(&id);
            tokio::select! {
                _ = shutdown.notified() => return,
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            }
            if let Err(error) = self
                .spawn_node(id.clone(), node.kind, node.group, node.spec)
                .await
            {
                self.tracker.failed(&id, error);
            }
        }
    }
}

#[async_trait]
impl Provisioner for StandaloneProvisioner {
    fn backend(&self) -> ProvisionBackend {
        ProvisionBackend::Standalone
    }

    fn supported_kinds(&self) -> Vec<ReplicaKind> {
        vec![
            ReplicaKind::Worker,
            ReplicaKind::Waker,
            ReplicaKind::Background,
        ]
    }

    async fn available(&self) -> bool {
        true
    }

    async fn list(&self) -> Result<Vec<ProvisionedGroup>, SendableError> {
        let nodes = self.nodes.lock().await;
        Ok(ReplicaKind::ALL
            .iter()
            .copied()
            .map(|kind| {
                let desired = nodes
                    .values()
                    .filter(|node| node.kind == kind && node.group == kind.as_str())
                    .count() as u32;
                let available = nodes
                    .iter()
                    .filter(|(id, node)| {
                        node.kind == kind
                            && node.group == kind.as_str()
                            && !node.task.is_finished()
                            && self.tracker.status(id).as_deref() == Some("running")
                    })
                    .count() as u32;
                ProvisionedGroup {
                    backend: ProvisionBackend::Standalone,
                    kind,
                    name: kind.as_str().into(),
                    desired,
                    available,
                    manageable: Self::manageable(kind),
                    min_desired: if kind == ReplicaKind::Webservice {
                        1
                    } else {
                        kind.min_desired()
                    },
                }
            })
            .collect())
    }

    async fn scale(
        &self,
        kind: ReplicaKind,
        desired: u32,
        spec: &NodeSpec,
    ) -> Result<ProvisionedGroup, SendableError> {
        if !Self::manageable(kind) {
            return Err(format!("standalone cannot scale {} nodes", kind.as_str()).into());
        }
        let group = spec.group.clone().unwrap_or_else(|| kind.as_str().into());
        let current = self
            .nodes
            .lock()
            .await
            .iter()
            .filter(|(_, node)| node.kind == kind && node.group == group)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if desired > current.len() as u32 {
            for _ in 0..(desired - current.len() as u32) {
                self.add_node(kind, group.clone(), spec.clone()).await?;
            }
        } else {
            for id in current
                .iter()
                .rev()
                .take(current.len().saturating_sub(desired as usize))
            {
                self.remove_node(id).await?;
            }
        }
        let nodes = self.nodes.lock().await;
        let desired_now = nodes
            .values()
            .filter(|node| node.kind == kind && node.group == group)
            .count() as u32;
        let available = nodes
            .iter()
            .filter(|(id, node)| {
                node.kind == kind
                    && node.group == group
                    && !node.task.is_finished()
                    && self.tracker.status(id).as_deref() == Some("running")
            })
            .count() as u32;
        Ok(ProvisionedGroup {
            backend: ProvisionBackend::Standalone,
            kind,
            name: group,
            desired: desired_now,
            available,
            manageable: true,
            min_desired: kind.min_desired(),
        })
    }

    async fn stop(&self, node_id: &str) -> Result<(), SendableError> {
        self.remove_node(node_id).await
    }
}

#[cfg(test)]
#[path = "provisioner_tests.rs"]
mod tests;
