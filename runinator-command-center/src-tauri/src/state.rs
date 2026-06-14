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
    /// rebuilt with a default `Authorization` header whenever the access token changes, so every
    /// request site picks up the credential without per-call plumbing.
    pub client: Arc<RwLock<Client>>,
}

impl CommandCenterState {
    pub fn new() -> Self {
        Self {
            service_url: Arc::new(RwLock::new(None)),
            discovery_started: Arc::new(AtomicBool::new(false)),
            client: Arc::new(RwLock::new(Client::new())),
        }
    }

    pub fn mark_discovery_started(&self) -> bool {
        self.discovery_started.swap(true, Ordering::SeqCst)
    }

    /// swap in a client that presents `token` as `Authorization: Bearer …` (or a plain client when
    /// `None`). called after login/refresh/logout.
    pub async fn set_access_token(&self, token: Option<String>) {
        let mut builder = Client::builder();
        if let Some(token) = token.filter(|value| !value.is_empty()) {
            if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(reqwest::header::AUTHORIZATION, value);
                builder = builder.default_headers(headers);
            }
        }
        if let Ok(client) = builder.build() {
            *self.client.write().await = client;
        }
    }
}
