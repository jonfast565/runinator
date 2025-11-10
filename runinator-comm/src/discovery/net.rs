use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use log::{debug, error, warn};
use tokio::{net::UdpSocket, time};

use crate::GossipMessage;

const BUFFER_SIZE: usize = 65_536;

/// Bind a UDP socket for gossip traffic and enable broadcast.
pub async fn bind_gossip_socket(bind_addr: &str, port: u16) -> std::io::Result<Arc<UdpSocket>> {
    let socket = Arc::new(UdpSocket::bind((bind_addr, port)).await?);
    socket.set_broadcast(true)?;
    Ok(socket)
}

/// Build the list of gossip broadcast targets using the standard defaults plus any extra entries.
pub fn gossip_targets<I, S>(gossip_port: u16, extra_targets: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut targets = vec![
        format!("255.255.255.255:{gossip_port}"),
        format!("127.0.0.1:{gossip_port}"),
    ];

    for target in extra_targets {
        let target = target.as_ref();
        if target.is_empty() {
            continue;
        }

        if target.contains(':') {
            targets.push(target.to_string());
        } else {
            targets.push(format!("{target}:{gossip_port}"));
        }
    }

    targets.sort();
    targets.dedup();
    targets
}

/// Spawn a background task that listens for gossip messages and hands them to the provided handler.
pub fn spawn_gossip_listener<H, Fut>(socket: Arc<UdpSocket>, mut handler: H)
where
    H: FnMut(GossipMessage, SocketAddr) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = vec![0u8; BUFFER_SIZE];

        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((len, addr)) => {
                    let payload = &buffer[..len];
                    let Ok(as_str) = std::str::from_utf8(payload) else {
                        warn!("Received invalid gossip payload from {}", addr);
                        continue;
                    };

                    match GossipMessage::from_json(as_str) {
                        Ok(message) => handler(message, addr).await,
                        Err(err) => warn!("Failed to parse gossip message: {}", err),
                    }
                }
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::ConnectionReset {
                        debug!(
                            "Gossip listener ignored connection reset (likely ICMP unreachable)"
                        );
                        continue;
                    }

                    error!("Error receiving gossip payload: {}", err);
                    time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });
}

/// Broadcast a gossip message to each of the provided targets, logging errors along the way.
pub async fn broadcast_gossip_message(
    socket: &UdpSocket,
    message: &GossipMessage,
    targets: &[String],
) {
    match message.to_json() {
        Ok(payload) => {
            for target in targets {
                if let Err(err) = socket.send_to(payload.as_bytes(), target).await {
                    warn!("Failed to send gossip to {target}: {err}");
                } else {
                    debug!("Sent gossip heartbeat to {}", target);
                }
            }
        }
        Err(err) => {
            warn!("Failed to serialize gossip announcement: {}", err);
        }
    }
}
