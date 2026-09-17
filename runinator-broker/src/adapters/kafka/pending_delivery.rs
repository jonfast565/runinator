#[allow(unused_imports)]
use super::*;

#[cfg(feature = "kafka")]
#[derive(Clone)]
pub(super) struct PendingDelivery {
    pub(super) consumer: Arc<rdkafka::consumer::StreamConsumer>,
    pub(super) topic: String,
    pub(super) partition: i32,
    pub(super) offset: i64,
}
