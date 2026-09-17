#[allow(unused_imports)]
use super::*;

pub(crate) struct ValidationGuard(pub(super) Arc<AtomicBool>);

impl ValidationGuard {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }

    pub fn store<S>(&self, inner: S) -> ValidationStore<S> {
        ValidationStore {
            inner,
            alive: self.0.clone(),
        }
    }
}

impl Drop for ValidationGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
