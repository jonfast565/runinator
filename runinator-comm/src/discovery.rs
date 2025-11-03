use std::{collections::HashMap, future::Future, net::SocketAddr, sync::Arc, time::Duration};

use chrono::{Duration as ChronoDuration, Utc};
use log::{debug, error, info, warn};
use tokio::{
    net::UdpSocket,
    sync::{Notify, RwLock},
    time,
};
use uuid::Uuid;

use crate::{GossipMessage, WebServiceAnnouncement};

const BUFFER_SIZE: usize = 65_536;

#[derive(Clone, Default)]
pub struct WebServiceDiscovery {
    services: Arc<RwLock<HashMap<Uuid, WebServiceAnnouncement>>>,
    notify: Arc<Notify>,
}

impl WebServiceDiscovery {
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
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
        guard
            .values()
            .cloned()
            .max_by_key(|svc| svc.last_heartbeat)
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

/// Bind a UDP socket for gossip traffic and enable broadcast.
pub async fn bind_gossip_socket(
    bind_addr: &str,
    port: u16,
) -> std::io::Result<Arc<UdpSocket>> {
    let socket = Arc::new(UdpSocket::bind((bind_addr, port)).await?);
    socket.set_broadcast(true)?;
    Ok(socket)
}

/// Build the list of gossip broadcast targets using the standard defaults plus any extra entries.
pub fn gossip_targets<I, S>(gossip_port: u16, extra_targets: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut targets = vec![
        format!("255.255.255.255:{gossip_port}"),
        format!("127.0.0.1:{gossip_port}"),
    ];

    for target in extra_targets {
        let target = target.as_ref();
        if target.is_empty() {
            continue;
        }

        if target.contains(':') {
            targets.push(target.to_string());
        } else {
            targets.push(format!("{target}:{gossip_port}"));
        }
    }

    targets.sort();
    targets.dedup();
    targets
}

/// Spawn a background task that listens for gossip messages and hands them to the provided handler.
pub fn spawn_gossip_listener<H, Fut>(socket: Arc<UdpSocket>, mut handler: H)
where
    H: FnMut(GossipMessage, SocketAddr) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = vec![0u8; BUFFER_SIZE];

        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((len, addr)) => {
                    let payload = &buffer[..len];
                    let Ok(as_str) = std::str::from_utf8(payload) else {
                        warn!("Received invalid gossip payload from {}", addr);
                        continue;
                    };

                    match GossipMessage::from_json(as_str) {
                        Ok(message) => handler(message, addr).await,
                        Err(err) => warn!("Failed to parse gossip message: {}", err),
                    }
                }
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::ConnectionReset {
                        debug!("Gossip listener ignored connection reset (likely ICMP unreachable)");
                        continue;
                    }

                    error!("Error receiving gossip payload: {}", err);
                    time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });
}

/// Broadcast a gossip message to each of the provided targets, logging errors along the way.
pub async fn broadcast_gossip_message(
    socket: &UdpSocket,
    message: &GossipMessage,
    targets: &[String],
) {
    match message.to_json() {
        Ok(payload) => {
            for target in targets {
                if let Err(err) = socket.send_to(payload.as_bytes(), target).await {
                    warn!("Failed to send gossip to {target}: {err}");
                } else {
                    debug!("Sent gossip heartbeat to {}", target);
                }
            }
        }
        Err(err) => {
            warn!("Failed to serialize gossip announcement: {}", err);
        }
    }
}

/// Ensure the service announcement carries an address, falling back to the peer IP if necessary.
pub fn apply_service_address(announcement: &mut WebServiceAnnouncement, fallback_ip: &str) {
    if announcement.address.is_empty() {
        announcement.address = fallback_ip.to_string();
    }
}

/// Construct the base URL for the announced web service.
pub fn web_service_base_url(service: &WebServiceAnnouncement) -> String {
    let mut base = format!("http://{}:{}", service.address, service.port);
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
