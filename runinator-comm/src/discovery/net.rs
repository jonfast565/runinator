use std::{
    collections::HashMap,
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use log::{debug, error, warn};
use tokio::{net::UdpSocket, time};

use crate::{GossipMessage, WireCodec};

const BUFFER_SIZE: usize = 65_536;

type SocketFuture<'a, T> = Pin<Box<dyn Future<Output = std::io::Result<T>> + Send + 'a>>;

/// UDP socket interface used by discovery. Production uses a Tokio socket.
/// [`VirtualNet`] provides deterministic broadcast tests without binding a host port.
pub trait UdpSocketLike: Send + Sync {
    fn recv_from<'a>(&'a self, buffer: &'a mut [u8]) -> SocketFuture<'a, (usize, SocketAddr)>;
    fn send_to<'a>(&'a self, payload: &'a [u8], target: &'a str) -> SocketFuture<'a, usize>;
}

impl UdpSocketLike for UdpSocket {
    fn recv_from<'a>(&'a self, buffer: &'a mut [u8]) -> SocketFuture<'a, (usize, SocketAddr)> {
        Box::pin(UdpSocket::recv_from(self, buffer))
    }

    fn send_to<'a>(&'a self, payload: &'a [u8], target: &'a str) -> SocketFuture<'a, usize> {
        Box::pin(UdpSocket::send_to(self, payload, target))
    }
}

type Datagram = (Vec<u8>, SocketAddr);

/// in-memory user datagram protocol (UDP) network with port-scoped IPv4 broadcast semantics.
#[derive(Clone, Default)]
pub struct VirtualNet {
    sockets: Arc<Mutex<HashMap<SocketAddr, tokio::sync::mpsc::UnboundedSender<Datagram>>>>,
}

impl VirtualNet {
    pub fn bind(&self, address: SocketAddr) -> Arc<VirtualUdpSocket> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.sockets
            .lock()
            .expect("virtual udp registry lock poisoned")
            .insert(address, sender);
        Arc::new(VirtualUdpSocket {
            address,
            net: self.clone(),
            receiver: tokio::sync::Mutex::new(receiver),
        })
    }
}

pub struct VirtualUdpSocket {
    address: SocketAddr,
    net: VirtualNet,
    receiver: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Datagram>>,
}

impl Drop for VirtualUdpSocket {
    fn drop(&mut self) {
        self.net
            .sockets
            .lock()
            .expect("virtual udp registry lock poisoned")
            .remove(&self.address);
    }
}

impl UdpSocketLike for VirtualUdpSocket {
    fn recv_from<'a>(&'a self, buffer: &'a mut [u8]) -> SocketFuture<'a, (usize, SocketAddr)> {
        Box::pin(async move {
            let (payload, sender) = self
                .receiver
                .lock()
                .await
                .recv()
                .await
                .ok_or_else(|| std::io::Error::other("virtual udp socket closed"))?;
            let len = payload.len().min(buffer.len());
            buffer[..len].copy_from_slice(&payload[..len]);
            Ok((len, sender))
        })
    }

    fn send_to<'a>(&'a self, payload: &'a [u8], target: &'a str) -> SocketFuture<'a, usize> {
        Box::pin(async move {
            let target = target.parse::<SocketAddr>().map_err(|err| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, err.to_string())
            })?;
            let broadcast = target.ip() == IpAddr::V4(Ipv4Addr::BROADCAST);
            let recipients = self
                .net
                .sockets
                .lock()
                .expect("virtual udp registry lock poisoned")
                .iter()
                .filter(|(address, _)| {
                    **address != self.address
                        && ((broadcast && address.port() == target.port()) || **address == target)
                })
                .map(|(_, sender)| sender.clone())
                .collect::<Vec<_>>();
            for recipient in recipients {
                let _ = recipient.send((payload.to_vec(), self.address));
            }
            Ok(payload.len())
        })
    }
}

/// Bind a user datagram protocol (UDP) socket for gossip traffic and enable broadcast.
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
pub fn spawn_gossip_listener<H, Fut>(socket: Arc<dyn UdpSocketLike>, mut handler: H)
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

                    match GossipMessage::from_wire(as_str) {
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
    socket: &dyn UdpSocketLike,
    message: &GossipMessage,
    targets: &[String],
) {
    match message.to_wire() {
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
