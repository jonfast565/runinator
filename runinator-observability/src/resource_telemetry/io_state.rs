#[allow(unused_imports)]
use super::*;

pub(super) struct IoState {
    pub(super) networks: Networks,
    pub(super) disks: Disks,
    pub(super) last_sample: Option<Instant>,
}
