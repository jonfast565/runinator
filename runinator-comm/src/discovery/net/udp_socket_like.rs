#[allow(unused_imports)]
use super::*;

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
