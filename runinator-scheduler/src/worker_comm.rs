use std::{collections::HashMap, convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::info;
use runinator_api::ServiceLocator;
use runinator_comm::{
    GossipMessage, WorkerAnnouncement, WorkerPeer,
    discovery::{
        WebServiceDiscovery, apply_service_address, bind_gossip_socket, spawn_gossip_listener,
    },
};
use runinator_models::errors::SendableError;
use tokio::{net::UdpSocket, sync::RwLock, time};
use uuid::Uuid;

use crate::config::Config;

#[derive(Clone, Debug)]
pub struct WorkerInfo {
    address: String,
    last_seen: DateTime<Utc>,
}

#[derive(Clone)]
pub struct WorkerManager {
    workers: Arc<RwLock<HashMap<Uuid, WorkerInfo>>>,
    service_discovery: WebServiceDiscovery,
}

impl WorkerManager {
    pub async fn new(config: &Config) -> Result<Self, SendableError> {
        let socket = bind_gossip_socket(config.gossip_bind.as_str(), config.gossip_port).await?;
        info!(
            "Listening for gossip on {}:{}",
            config.gossip_bind, config.gossip_port
        );

        let manager = WorkerManager {
            workers: Arc::new(RwLock::new(HashMap::new())),
            service_discovery: WebServiceDiscovery::new(),
        };

        manager.spawn_listener(socket.clone());
        manager.spawn_cleanup();

        Ok(manager)
    }

    pub fn service_registry(&self) -> &WebServiceDiscovery {
        &self.service_discovery
    }

    fn spawn_listener(&self, socket: Arc<UdpSocket>) {
        let workers = self.workers.clone();
        let service_discovery = self.service_discovery.clone();
        spawn_gossip_listener(socket, move |message, peer_addr| {
            let workers = workers.clone();
            let service_discovery = service_discovery.clone();

            async move {
                match message {
                    GossipMessage::Worker { worker } => {
                        update_worker(workers.clone(), worker, peer_addr).await;
                    }
                    GossipMessage::WebService { mut service } => {
                        let fallback = peer_addr.ip().to_string();
                        apply_service_address(&mut service, &fallback);
                        service_discovery.register(service).await;
                    }
                }
            }
        });
    }

    fn spawn_cleanup(&self) {
        let workers = self.workers.clone();
        let service_discovery = self.service_discovery.clone();
        tokio::spawn(async move {
            let stale_after = chrono::Duration::seconds(60);
            loop {
                time::sleep(Duration::from_secs(15)).await;
                let mut workers_guard = workers.write().await;
                let now = Utc::now();

                let workers_before = workers_guard.len();
                workers_guard.retain(|_, info| now - info.last_seen <= stale_after);
                let removed_workers = workers_before - workers_guard.len();
                if removed_workers > 0 {
                    info!("Removed {removed_workers} stale worker(s) from registry");
                }

                drop(workers_guard);

                service_discovery.prune_stale(stale_after).await;
            }
        });
    }

    pub async fn wait_for_service_url(&self) -> Result<String, SendableError> {
        Ok(self.service_registry().wait_for_service_url().await)
    }

    pub async fn current_service_url(&self) -> Option<String> {
        self.service_registry().current_service_url().await
    }
}

impl AsRef<WebServiceDiscovery> for WorkerManager {
    fn as_ref(&self) -> &WebServiceDiscovery {
        self.service_registry()
    }
}

#[async_trait]
impl ServiceLocator for WorkerManager {
    type Error = Infallible;

    async fn wait_for_service_url(&self) -> Result<String, Self::Error> {
        Ok(self.service_registry().wait_for_service_url().await)
    }
}

async fn update_worker(
    workers: Arc<RwLock<HashMap<Uuid, WorkerInfo>>>,
    announcement: WorkerAnnouncement,
    peer_addr: SocketAddr,
) {
    {
        let mut guard = workers.write().await;
        let address = if announcement.address.is_empty() {
            peer_addr.ip().to_string()
        } else {
            announcement.address.clone()
        };

        let entry = guard
            .entry(announcement.worker_id)
            .or_insert_with(|| WorkerInfo {
                address: address.clone(),
                last_seen: announcement.last_heartbeat,
            });

        entry.address = address;
        entry.last_seen = announcement.last_heartbeat;
    }

    for peer in announcement.known_peers {
        update_peer(workers.clone(), peer).await;
    }
}

async fn update_peer(workers: Arc<RwLock<HashMap<Uuid, WorkerInfo>>>, peer: WorkerPeer) {
    let mut guard = workers.write().await;
    let entry = guard.entry(peer.worker_id).or_insert_with(|| WorkerInfo {
        address: peer.address.clone(),
        last_seen: peer.last_heartbeat,
    });

    entry.address = peer.address;
    entry.last_seen = peer.last_heartbeat;
}
