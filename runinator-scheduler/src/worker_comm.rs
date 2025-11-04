use std::{
    convert::Infallible,
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use log::{debug, error, info};
use runinator_comm::{
    discovery::{
        apply_service_address, bind_gossip_socket, spawn_gossip_listener, WebServiceDiscovery,
    },
    GossipMessage, TaskCommand, TaskResult, WorkerAnnouncement, WorkerPeer,
};
use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
};
use runinator_api::ServiceLocator;
use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpStream, UdpSocket},
    sync::RwLock,
    time,
};
use uuid::Uuid;

use crate::config::Config;

#[derive(Clone, Debug)]
pub struct WorkerInfo {
    id: Uuid,
    address: String,
    command_port: u16,
    last_seen: DateTime<Utc>,
    last_failure: Option<Instant>,
}

impl WorkerInfo {
    fn socket_addr(&self) -> String {
        format!("{}:{}", self.address, self.command_port)
    }
}

#[derive(Clone)]
pub struct WorkerManager {
    workers: Arc<RwLock<HashMap<Uuid, WorkerInfo>>>,
    service_discovery: WebServiceDiscovery,
}

impl WorkerManager {
    pub async fn new(config: &Config) -> Result<Self, SendableError> {
        let socket =
            bind_gossip_socket(config.gossip_bind.as_str(), config.gossip_port).await?;
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

    pub async fn dispatch_task(
        &self,
        task: &ScheduledTask,
        timeout: Duration,
        retries: u8,
    ) -> Result<TaskResult, SendableError> {
        let command = TaskCommand {
            command_id: Uuid::new_v4(),
            task: task.clone(),
        };

        let mut last_error: Option<SendableError> = None;

        for attempt in 0..retries.max(1) {
            let worker = match self.select_worker().await {
                Some(worker) => worker,
                None => {
                    return Err(Box::new(RuntimeError::new(
                        "scheduler.no_workers".into(),
                        "No available workers discovered".into(),
                    )));
                }
            };

            debug!(
                "Dispatching task {} to worker {} (attempt {}/{})",
                task.id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                worker.id,
                attempt + 1,
                retries.max(1)
            );

            match self.send_command(&worker, command.clone(), timeout).await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    error!(
                        "Worker {} dispatch failed: {}. Trying another worker...",
                        worker.id, err
                    );
                    last_error = Some(err);
                    self.mark_failure(worker.id).await;
                    continue;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            Box::new(RuntimeError::new(
                "scheduler.dispatch.failed".into(),
                "Task dispatch failed without specific error".into(),
            ))
        }))
    }

    pub async fn wait_for_service_url(&self) -> Result<String, SendableError> {
        Ok(self.service_registry().wait_for_service_url().await)
    }

    pub async fn current_service_url(&self) -> Option<String> {
        self.service_registry().current_service_url().await
    }

    async fn select_worker(&self) -> Option<WorkerInfo> {
        let guard = self.workers.read().await;
        guard
            .values()
            .filter(|info| {
                if let Some(last_failure) = info.last_failure {
                    last_failure.elapsed() > Duration::from_secs(10)
                } else {
                    true
                }
            })
            .cloned()
            .max_by_key(|info| info.last_seen)
    }

    async fn mark_failure(&self, worker_id: Uuid) {
        if let Some(worker) = self.workers.write().await.get_mut(&worker_id) {
            worker.last_failure = Some(Instant::now());
        }
    }

    async fn send_command(
        &self,
        worker: &WorkerInfo,
        command: TaskCommand,
        timeout: Duration,
    ) -> Result<TaskResult, SendableError> {
        let target = worker.socket_addr();
        let payload = command
            .to_json()
            .map_err(|err| -> SendableError { Box::new(err) })?;

        let connect_future = TcpStream::connect(target.clone());
        let mut stream = time::timeout(timeout, connect_future)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?
            .map_err(|err| -> SendableError { Box::new(err) })?;

        stream
            .write_all(payload.as_bytes())
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        stream
            .flush()
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();

        time::timeout(timeout, reader.read_line(&mut response))
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?
            .map_err(|err| -> SendableError { Box::new(err) })?;

        let trimmed = response.trim();
        let result =
            TaskResult::from_json(trimmed).map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(result)
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
                id: announcement.worker_id,
                address: address.clone(),
                command_port: announcement.command_port,
                last_seen: announcement.last_heartbeat,
                last_failure: None,
            });

        entry.address = address;
        entry.command_port = announcement.command_port;
        entry.last_seen = announcement.last_heartbeat;
        entry.last_failure = None;
    }

    for peer in announcement.known_peers {
        update_peer(workers.clone(), peer).await;
    }
}

async fn update_peer(workers: Arc<RwLock<HashMap<Uuid, WorkerInfo>>>, peer: WorkerPeer) {
    let mut guard = workers.write().await;
    let entry = guard.entry(peer.worker_id).or_insert_with(|| WorkerInfo {
        id: peer.worker_id,
        address: peer.address.clone(),
        command_port: peer.command_port,
        last_seen: peer.last_heartbeat,
        last_failure: None,
    });

    entry.address = peer.address;
    entry.command_port = peer.command_port;
    entry.last_seen = peer.last_heartbeat;
}

