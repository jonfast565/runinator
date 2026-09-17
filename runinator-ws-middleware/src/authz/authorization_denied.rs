#[allow(unused_imports)]
use super::*;

pub struct AuthorizationDenied(pub(super) Box<Reply>);

impl AuthorizationDenied {
    pub(super) fn forbidden() -> Self {
        Self(Box::new(forbidden()))
    }

    pub fn into_reply(self) -> Reply {
        *self.0
    }
}

impl IntoReply for AuthorizationDenied {
    fn into_reply(self) -> Reply {
        self.into_reply()
    }
}
