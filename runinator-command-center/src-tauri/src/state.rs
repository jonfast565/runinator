use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use reqwest::Client;
use tokio::sync::RwLock;

use crate::worker::EmbeddedWorker;

#[derive(Clone)]
pub struct CommandCenterState {
    pub service_url: Arc<RwLock<Option<String>>>,
    pub discovery_started: Arc<AtomicBool>,
    /// rebuilt with a default `Authorization` header whenever the access token changes, so every
    /// request site picks up the credential without per-call plumbing.
    pub client: Arc<RwLock<Client>>,
    /// the raw access token, retained so the embedded worker can build its own api/broker clients.
    pub access_token: Arc<RwLock<Option<String>>>,
    /// lifecycle of the optional in-process desktop worker.
    pub embedded_worker: Arc<RwLock<EmbeddedWorker>>,
}

impl CommandCenterState {
    pub fn new() -> Self {
        Self {
            service_url: Arc::new(RwLock::new(None)),
            discovery_started: Arc::new(AtomicBool::new(false)),
            client: Arc::new(RwLock::new(Client::new())),
            access_token: Arc::new(RwLock::new(None)),
            embedded_worker: Arc::new(RwLock::new(EmbeddedWorker::default())),
        }
    }

    pub fn mark_discovery_started(&self) -> bool {
        self.discovery_started.swap(true, Ordering::SeqCst)
    }

    /// swap in a client that presents `token` as `Authorization: Bearer …` (or a plain client when
    /// `None`). called after login/refresh/logout. the raw token is retained for the embedded worker.
    pub async fn set_access_token(&self, token: Option<String>) {
        let normalized = token.filter(|value| !value.is_empty());
        let mut builder = Client::builder();
        if let Some(token) = normalized.as_deref() {
            if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(reqwest::header::AUTHORIZATION, value);
                builder = builder.default_headers(headers);
            }
        }
        if let Ok(client) = builder.build() {
            *self.client.write().await = client;
        }
        *self.access_token.write().await = normalized;
    }
}
