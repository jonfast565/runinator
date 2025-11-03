use std::sync::Arc;

use runinator_comm::discovery::{
    apply_service_address, bind_gossip_socket, spawn_gossip_listener, WebServiceDiscovery,
};
use tokio::net::UdpSocket;

use crate::config::Config;

#[derive(Clone)]
pub struct ServiceDiscovery {
    inner: WebServiceDiscovery,
}

impl ServiceDiscovery {
    pub async fn new(config: &Config) -> Result<Self, std::io::Error> {
        let socket = bind_gossip_socket(config.gossip_bind.as_str(), config.gossip_port).await?;

        let service = ServiceDiscovery {
            inner: WebServiceDiscovery::new(),
        };
        service.spawn_listener(socket);
        Ok(service)
    }

    fn spawn_listener(&self, socket: Arc<UdpSocket>) {
        let discovery = self.inner.clone();

        spawn_gossip_listener(socket, move |message, addr| {
            let discovery = discovery.clone();

            async move {
                if let runinator_comm::GossipMessage::WebService { mut service } = message {
                    let fallback = addr.ip().to_string();
                    apply_service_address(&mut service, &fallback);
                    discovery.register(service).await;
                }
            }
        });
    }
}

impl std::ops::Deref for ServiceDiscovery {
    type Target = WebServiceDiscovery;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl AsRef<WebServiceDiscovery> for ServiceDiscovery {
    fn as_ref(&self) -> &WebServiceDiscovery {
        &self.inner
    }
}
