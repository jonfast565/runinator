use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use reqwest::Client;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct CommandCenterState {
    pub service_url: Arc<RwLock<Option<String>>>,
    pub discovery_started: Arc<AtomicBool>,
    pub client: Client,
}

impl CommandCenterState {
    pub fn new() -> Self {
        Self {
            service_url: Arc::new(RwLock::new(None)),
            discovery_started: Arc::new(AtomicBool::new(false)),
            client: Client::new(),
        }
    }

    pub fn mark_discovery_started(&self) -> bool {
        self.discovery_started.swap(true, Ordering::SeqCst)
    }
}
