#[allow(unused_imports)]
use super::*;

pub trait LineSink: Send + Sync {
    fn line(&self, stream: Stream, text: &str);
}
