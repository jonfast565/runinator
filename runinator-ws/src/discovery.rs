use std::{sync::Arc, time::Duration};

use log::{error, info, warn};
use runinator_comm::{
    discovery::{broadcast_gossip_message, gossip_targets},
    GossipMessage, WebServiceAnnouncement,
};

use tokio::{net::UdpSocket, sync::Notify, time};
use uuid::Uuid;

pub(crate) fn spawn_gossip_advertiser_ws(
    service_id: Uuid,
    bind_addr: String,
    gossip_port: u16,
    extra_targets: Vec<String>,
    announce_address: String,
    announce_base_path: String,
    interval_seconds: u64,
    notify: Arc<Notify>,
    service_port: u16,
) {
    tokio::spawn(async move {
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
                _ = notify.notified() => break,
                _ = ticker.tick() => {
                    let announcement = WebServiceAnnouncement {
                        service_id,
                        address: announce_address.clone(),
                        port: service_port,
                        base_path: Some(announce_base_path.clone()),
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
    });
}
