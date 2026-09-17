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

mod web_service_discovery;
pub use web_service_discovery::WebServiceDiscovery;

mod web_service_advertiser_config;
pub use web_service_advertiser_config::WebServiceAdvertiserConfig;
