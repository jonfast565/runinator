#[allow(unused_imports)]
use super::*;

pub trait IntoReply {
    fn into_reply(self) -> Reply;
}

impl IntoReply for Reply {
    fn into_reply(self) -> Reply {
        self
    }
}
