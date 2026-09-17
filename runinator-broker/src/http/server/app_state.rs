#[allow(unused_imports)]
use super::*;

pub(super) struct AppState<B> {
    pub(super) broker: Arc<B>,
}

impl<B> Clone for AppState<B> {
    fn clone(&self) -> Self {
        Self {
            broker: Arc::clone(&self.broker),
        }
    }
}
