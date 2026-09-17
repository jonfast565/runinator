#[allow(unused_imports)]
use super::*;

/// in-memory user datagram protocol (UDP) network with port-scoped IPv4 broadcast semantics.
#[derive(Clone, Default)]
pub struct VirtualNet {
    pub(super) sockets:
        Arc<Mutex<HashMap<SocketAddr, tokio::sync::mpsc::UnboundedSender<Datagram>>>>,
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
