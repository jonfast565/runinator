#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct Ports {
    pub(super) broker: u16,
    pub(super) web: u16,
    pub(super) web_gossip: u16,
    pub(super) scheduler_gossip: u16,
}

impl Ports {
    pub(super) fn allocate() -> std::io::Result<Self> {
        Ok(Self {
            broker: free_port()?,
            web: free_port()?,
            web_gossip: free_port()?,
            scheduler_gossip: free_port()?,
        })
    }
}
