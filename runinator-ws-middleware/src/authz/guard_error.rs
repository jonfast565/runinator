#[allow(unused_imports)]
use super::*;

pub struct GuardError(pub(super) Box<Reply>);

impl From<Reply> for GuardError {
    fn from(reply: Reply) -> Self {
        Self(Box::new(reply))
    }
}

impl From<AuthorizationDenied> for GuardError {
    fn from(error: AuthorizationDenied) -> Self {
        Self(error.0)
    }
}

impl IntoReply for GuardError {
    fn into_reply(self) -> Reply {
        *self.0
    }
}
