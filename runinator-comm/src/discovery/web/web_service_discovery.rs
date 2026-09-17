#[allow(unused_imports)]
use super::*;

#[derive(Clone, Default)]
pub struct WebServiceDiscovery {
    pub(super) services: Arc<tokio::sync::RwLock<HashMap<Uuid, WebServiceAnnouncement>>>,
    pub(super) notify: Arc<Notify>,
}

impl WebServiceDiscovery {
    pub fn new() -> Self {
        Self {
            services: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    pub async fn register(&self, mut announcement: WebServiceAnnouncement) -> bool {
        if let Some(path) = announcement.base_path.take() {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                announcement.base_path = Some(trimmed.to_string());
            }
        }

        let mut guard = self.services.write().await;
        let service_id = announcement.service_id;
        let is_new = !guard.contains_key(&service_id);

        guard.insert(service_id, announcement.clone());

        if is_new {
            info!(
                "Discovered Runinator Web Service at {}:{}",
                announcement.address, announcement.port
            );
            self.notify.notify_waiters();
        }

        is_new
    }

    pub async fn current_service(&self) -> Option<WebServiceAnnouncement> {
        let guard = self.services.read().await;
        guard.values().cloned().max_by_key(|svc| svc.last_heartbeat)
    }

    /// newest candidate belonging to the cluster authorized by an enrollment token. discovery
    /// alone is never identity: callers without a bound cluster id must present candidates to an
    /// operator instead of automatically choosing one.
    pub async fn current_service_for_cluster(
        &self,
        cluster_id: Uuid,
    ) -> Option<WebServiceAnnouncement> {
        let guard = self.services.read().await;
        guard
            .values()
            .filter(|service| service.cluster_id == cluster_id)
            .cloned()
            .max_by_key(|service| service.last_heartbeat)
    }

    pub async fn candidates(&self) -> Vec<WebServiceAnnouncement> {
        self.services.read().await.values().cloned().collect()
    }

    pub async fn current_service_url(&self) -> Option<String> {
        self.current_service()
            .await
            .map(|svc| web_service_base_url(&svc))
    }

    pub async fn wait_for_service_url(&self) -> String {
        loop {
            if let Some(url) = self.current_service_url().await {
                return url;
            }
            self.notify.notified().await;
        }
    }

    pub async fn wait_for_cluster_url(&self, cluster_id: Uuid) -> String {
        loop {
            if let Some(service) = self.current_service_for_cluster(cluster_id).await {
                return web_service_base_url(&service);
            }
            self.notify.notified().await;
        }
    }

    pub async fn prune_stale(&self, max_age: ChronoDuration) -> usize {
        let mut guard = self.services.write().await;
        let before = guard.len();
        let now = Utc::now();
        guard.retain(|_, svc| now - svc.last_heartbeat <= max_age);
        let removed = before - guard.len();
        if removed > 0 {
            info!("Removed {removed} stale service announcement(s)");
        }
        removed
    }
}
