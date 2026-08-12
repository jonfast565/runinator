use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{Duration as ChronoDuration, Utc};
use log::{error, info, warn};
use tokio::{
    net::UdpSocket,
    sync::Notify,
    task::JoinHandle,
    time::{self},
};
use uuid::Uuid;

use crate::{GossipMessage, WebServiceAnnouncement};

use super::net::{
    UdpSocketLike, bind_gossip_socket, broadcast_gossip_message, gossip_targets,
    spawn_gossip_listener,
};

#[derive(Clone, Default)]
pub struct WebServiceDiscovery {
    services: Arc<tokio::sync::RwLock<HashMap<Uuid, WebServiceAnnouncement>>>,
    notify: Arc<Notify>,
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

/// Start listening for web service gossip announcements on the provided bind address.
pub async fn start_web_service_listener(
    bind_addr: &str,
    port: u16,
) -> std::io::Result<WebServiceDiscovery> {
    let socket = bind_gossip_socket(bind_addr, port).await?;
    Ok(spawn_web_service_listener(socket))
}

/// Spawn a listener for web service gossip announcements on an already-bound socket.
pub fn spawn_web_service_listener(socket: Arc<UdpSocket>) -> WebServiceDiscovery {
    spawn_web_service_listener_with_socket(socket)
}

/// socket-abstracted listener used by the virtual network test harness.
pub fn spawn_web_service_listener_with_socket(
    socket: Arc<dyn UdpSocketLike>,
) -> WebServiceDiscovery {
    let discovery = WebServiceDiscovery::new();
    attach_web_service_listener(socket, discovery.clone());
    discovery
}

fn attach_web_service_listener(socket: Arc<dyn UdpSocketLike>, discovery: WebServiceDiscovery) {
    spawn_gossip_listener(socket, move |message, addr| {
        let discovery = discovery.clone();

        async move {
            if let GossipMessage::WebService { mut service } = message {
                let fallback = addr.ip().to_string();
                apply_service_address(&mut service, &fallback);
                discovery.register(service).await;
            }
        }
    });
}

/// Configuration for advertising a web service over gossip.
pub struct WebServiceAdvertiserConfig {
    pub service_id: Uuid,
    pub bind_addr: String,
    pub gossip_port: u16,
    pub extra_targets: Vec<String>,
    pub announce_address: String,
    pub announce_base_path: String,
    pub announce_scheme: String,
    pub announce_relay_path: String,
    pub cluster_id: Uuid,
    pub enrollment_enabled: bool,
    pub spki_pin: Option<String>,
    pub version: Option<String>,
    pub interval_seconds: u64,
    pub shutdown: Arc<Notify>,
    pub service_port: u16,
}

/// Spawn a gossip advertiser for the Runinator web service.
pub fn spawn_web_service_advertiser(config: WebServiceAdvertiserConfig) -> JoinHandle<()> {
    tokio::spawn(async move {
        let WebServiceAdvertiserConfig {
            service_id,
            bind_addr,
            gossip_port,
            extra_targets,
            announce_address,
            announce_base_path,
            announce_scheme,
            announce_relay_path,
            cluster_id,
            enrollment_enabled,
            spki_pin,
            version,
            interval_seconds,
            shutdown,
            service_port,
        } = config;

        let socket = match UdpSocket::bind((bind_addr.as_str(), 0)).await {
            Ok(socket) => {
                if let Err(err) = socket.set_broadcast(true) {
                    warn!("Unable to enable broadcast on gossip socket: {}", err);
                }
                socket
            }
            Err(err) => {
                error!("Failed to bind gossip socket: {}", err);
                return;
            }
        };

        let targets = gossip_targets(gossip_port, extra_targets);

        let interval = Duration::from_secs(interval_seconds.max(1));
        let mut ticker = time::interval(interval);
        info!(
            "Advertising Runinator web service via gossip on UDP port {}",
            gossip_port
        );

        loop {
            tokio::select! {
                _ = shutdown.notified() => break,
                _ = ticker.tick() => {
                    let announcement = WebServiceAnnouncement {
                        service_id,
                        address: announce_address.clone(),
                        port: service_port,
                        base_path: Some(announce_base_path.clone()),
                        scheme: announce_scheme.clone(),
                        relay_path: announce_relay_path.clone(),
                        cluster_id,
                        enrollment_enabled,
                        spki_pin: spki_pin.clone(),
                        version: version.clone(),
                        last_heartbeat: chrono::Utc::now(),
                    };

                    let message = GossipMessage::WebService {
                        service: announcement,
                    };
                    broadcast_gossip_message(&socket, &message, &targets).await;
                }
            }
        }
        info!("Stopped gossip advertisements");
    })
}

/// Ensure the service announcement carries an address, falling back to the peer IP if necessary.
pub fn apply_service_address(announcement: &mut WebServiceAnnouncement, fallback_ip: &str) {
    if announcement.address.is_empty() {
        announcement.address = fallback_ip.to_string();
    }
}

/// Construct the base URL for the announced web service.
pub fn web_service_base_url(service: &WebServiceAnnouncement) -> String {
    let scheme = match service.scheme.as_str() {
        "https" => "https",
        _ => "http",
    };
    let mut base = format!("{scheme}://{}:{}", service.address, service.port);
    if let Some(path) = service.base_path.as_ref() {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            if trimmed.starts_with('/') {
                base.push_str(trimmed);
            } else {
                base.push('/');
                base.push_str(trimmed);
            }
        }
    }
    base
}

/// deterministic fallback identity for deployments that have not configured an explicit cluster
/// id. operators should set a stable id when the public enrollment URL differs from gossip.
pub fn cluster_id_for_service_url(service_url: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        service_url.trim_end_matches('/').as_bytes(),
    )
}
