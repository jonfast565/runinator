use std::{convert::Infallible, env, sync::Arc};

use anyhow::{Context, Result};
use runinator_api::BlockingServiceLocator;
use runinator_comm::{
    discovery::{
        apply_service_address, bind_gossip_socket, spawn_gossip_listener, WebServiceDiscovery,
    },
    GossipMessage,
};
use tokio::runtime::Runtime;

/// Blocking locator that listens for Runinator web service gossip announcements.
#[derive(Clone)]
pub struct GossipLocator {
    discovery: Arc<WebServiceDiscovery>,
    runtime: Arc<Runtime>,
}

impl GossipLocator {
    /// Build a locator using environment variables for gossip configuration.
    ///
    /// `RUNINATOR_GOSSIP_BIND` defaults to `0.0.0.0` and `RUNINATOR_GOSSIP_PORT`
    /// defaults to `5000` when not provided.
    pub fn from_env() -> Result<Self> {
        let bind = env::var("RUNINATOR_GOSSIP_BIND").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("RUNINATOR_GOSSIP_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(5000);

        Self::new(bind, port)
    }

    /// Construct a locator that listens for gossip announcements on the provided endpoint.
    pub fn new(gossip_bind: String, gossip_port: u16) -> Result<Self> {
        let runtime = Arc::new(Runtime::new().context("failed to initialize Tokio runtime")?);
        let discovery = Arc::new(WebServiceDiscovery::new());

        {
            let bind_addr = gossip_bind;
            let discovery_clone = discovery.clone();
            runtime
                .block_on(async move {
                    let socket = bind_gossip_socket(bind_addr.as_str(), gossip_port)
                        .await
                        .context("failed to bind gossip socket")?;

                    spawn_gossip_listener(socket, move |message, addr| {
                        let discovery = discovery_clone.clone();
                        async move {
                            if let GossipMessage::WebService { mut service } = message {
                                let fallback = addr.ip().to_string();
                                apply_service_address(&mut service, &fallback);
                                discovery.register(service).await;
                            }
                        }
                    });
                    Ok::<(), anyhow::Error>(())
                })
                .context("failed while initializing gossip listener")?;
        }

        Ok(Self { discovery, runtime })
    }
}

impl BlockingServiceLocator for GossipLocator {
    type Error = Infallible;

    fn wait_for_service_url(&self) -> Result<String, Self::Error> {
        Ok(self
            .runtime
            .block_on(self.discovery.wait_for_service_url()))
    }
}
