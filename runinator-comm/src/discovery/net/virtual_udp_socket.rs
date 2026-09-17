#[allow(unused_imports)]
use super::*;

pub struct VirtualUdpSocket {
    pub(super) address: SocketAddr,
    pub(super) net: VirtualNet,
    pub(super) receiver: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Datagram>>,
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
