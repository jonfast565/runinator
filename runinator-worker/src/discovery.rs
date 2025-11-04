use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::Utc;
use runinator_comm::{
    discovery::{
        apply_service_address, bind_gossip_socket, broadcast_gossip_message, gossip_targets,
        spawn_gossip_listener, WebServiceDiscovery,
    },
    GossipMessage, WorkerAnnouncement, WorkerPeer,
};
use tokio::{
    net::UdpSocket,
    sync::RwLock,
    time::{self, MissedTickBehavior},
};
use uuid::Uuid;

use crate::config::Config;

pub struct DiscoveryService {
    worker_id: Uuid,
    announce_address: String,
    command_port: u16,
    broadcast_targets: Vec<String>,
    socket: Arc<UdpSocket>,
    known_workers: Arc<RwLock<HashMap<Uuid, WorkerPeer>>>,
    service_discovery: WebServiceDiscovery,
}

impl DiscoveryService {
    pub async fn new(config: &Config) -> Result<Self, std::io::Error> {
        let socket = bind_gossip_socket(config.gossip_bind.as_str(), config.gossip_port).await?;

        let targets =
            gossip_targets(config.gossip_port, config.gossip_targets.iter());

        Ok(Self {
            worker_id: config.worker_id,
            announce_address: config.announce_address.clone(),
            command_port: config.command_port,
            broadcast_targets: targets,
            socket,
            known_workers: Arc::new(RwLock::new(HashMap::new())),
            service_discovery: WebServiceDiscovery::new(),
        })
    }

    pub fn start(&self, interval: Duration) {
        self.spawn_listener();
        self.spawn_broadcaster(interval);
    }

    fn spawn_listener(&self) {
        let socket = Arc::clone(&self.socket);
        let known_workers = Arc::clone(&self.known_workers);
        let service_discovery = self.service_discovery.clone();
        let self_id = self.worker_id;

        spawn_gossip_listener(socket, move |message, addr| {
            let known_workers = Arc::clone(&known_workers);
            let service_discovery = service_discovery.clone();

            async move {
                match message {
                    GossipMessage::Worker { worker } => {
                        if worker.worker_id != self_id {
                            update_known_workers(
                                &known_workers,
                                worker,
                                addr.ip().to_string(),
                            )
                            .await;
                        }
                    }
                    GossipMessage::WebService { mut service } => {
                        let fallback = addr.ip().to_string();
                        apply_service_address(&mut service, &fallback);
                        service_discovery.register(service).await;
                    }
                }
            }
        });
    }

    fn spawn_broadcaster(&self, interval: Duration) {
        let socket = Arc::clone(&self.socket);
        let known_workers = Arc::clone(&self.known_workers);
        let targets = self.broadcast_targets.clone();
        let worker_id = self.worker_id;
        let address = self.announce_address.clone();
        let command_port = self.command_port;

        tokio::spawn(async move {
            let mut ticker = time::interval(interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let peers = {
                    let guard = known_workers.read().await;
                    guard
                        .values()
                        .cloned()
                        .filter(|peer| peer.worker_id != worker_id)
                        .collect::<Vec<_>>()
                };

                let announcement = WorkerAnnouncement {
                    worker_id,
                    address: address.clone(),
                    command_port,
                    last_heartbeat: Utc::now(),
                    known_peers: peers,
                };

                let message = GossipMessage::Worker {
                    worker: announcement,
                };
                broadcast_gossip_message(&socket, &message, &targets).await;
            }
        });
    }

    //pub async fn latest_web_service(&self) -> Option<WebServiceAnnouncement> {
    //    self.service_discovery.current_service().await
    //}
}

async fn update_known_workers(
    known_workers: &Arc<RwLock<HashMap<Uuid, WorkerPeer>>>,
    announcement: WorkerAnnouncement,
    fallback_ip: String,
) {
    let mut guard = known_workers.write().await;
    let address = if announcement.address.is_empty() {
        fallback_ip
    } else {
        announcement.address.clone()
    };

    guard.insert(
        announcement.worker_id,
        WorkerPeer {
            worker_id: announcement.worker_id,
            address,
            command_port: announcement.command_port,
            last_heartbeat: announcement.last_heartbeat,
        },
    );

    for peer in announcement.known_peers {
        if peer.worker_id == announcement.worker_id {
            continue;
        }
        guard.insert(peer.worker_id, peer);
    }
}
