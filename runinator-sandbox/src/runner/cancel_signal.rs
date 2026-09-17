#[allow(unused_imports)]
use super::*;

pub trait CancelSignal: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

impl<F> CancelSignal for F
where
    F: Fn() -> bool + Send + Sync,
{
    fn is_cancelled(&self) -> bool {
        self()
    }
}
