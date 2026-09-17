#[allow(unused_imports)]
use super::*;

/// a borrowed `lapin::Channel` plus the qos/declare/publish operations run against it. every call
/// site already holds the channel as a short-lived local, so borrowing here avoids cloning the
/// (cheap but pointless) handle just to name a receiver.
#[cfg(feature = "rabbitmq")]
pub(super) struct RabbitChannel<'a>(pub(super) &'a lapin::Channel);

#[cfg(feature = "rabbitmq")]
impl RabbitChannel<'_> {
    // cap unacked deliveries per consumer on this channel; a zero prefetch leaves rabbitmq unlimited.
    pub(super) async fn apply_qos(&self, config: &RabbitMqBrokerConfig) -> Result<(), BrokerError> {
        if config.prefetch_count == 0 {
            return Ok(());
        }
        self.0
            .basic_qos(
                config.prefetch_count,
                lapin::options::BasicQosOptions::default(),
            )
            .await
            .map_err(rabbitmq_error("basic_qos"))
    }

    pub(super) async fn declare_queue(&self, queue: &str) -> Result<(), BrokerError> {
        self.0
            .queue_declare(
                queue.into(),
                lapin::options::QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .map(|_| ())
            .map_err(rabbitmq_error("queue_declare"))
    }

    pub(super) async fn declare_fanout_exchange(&self, exchange: &str) -> Result<(), BrokerError> {
        self.0
            .exchange_declare(
                exchange.into(),
                lapin::ExchangeKind::Fanout,
                lapin::options::ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .map(|_| ())
            .map_err(rabbitmq_error("exchange_declare"))
    }

    pub(super) async fn publish_fanout(
        &self,
        exchange: &str,
        payload: String,
    ) -> Result<(), BrokerError> {
        self.0
            .basic_publish(
                exchange.into(),
                "".into(),
                lapin::options::BasicPublishOptions::default(),
                payload.as_bytes(),
                lapin::BasicProperties::default(),
            )
            .await
            .map_err(rabbitmq_error("publish_event"))?
            .await
            .map_err(rabbitmq_error("publish_event_confirm"))?;
        Ok(())
    }
    pub(super) async fn publish(
        &self,
        queue: &str,
        key: &str,
        payload: String,
    ) -> Result<(), BrokerError> {
        self.0
            .basic_publish(
                "".into(),
                queue.into(),
                lapin::options::BasicPublishOptions::default(),
                payload.as_bytes(),
                lapin::BasicProperties::default()
                    .with_delivery_mode(2)
                    .with_message_id(key.into()),
            )
            .await
            .map_err(rabbitmq_error("publish"))?
            .await
            .map_err(rabbitmq_error("publish_confirm"))?;
        Ok(())
    }

    pub(super) async fn publish_expiring(
        &self,
        queue: &str,
        key: &str,
        payload: String,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), BrokerError> {
        let ttl_ms = (expires_at - chrono::Utc::now()).num_milliseconds();
        if ttl_ms <= 0 {
            return Ok(());
        }
        self.0
            .basic_publish(
                "".into(),
                queue.into(),
                lapin::options::BasicPublishOptions::default(),
                payload.as_bytes(),
                lapin::BasicProperties::default()
                    .with_delivery_mode(2)
                    .with_message_id(key.into())
                    .with_expiration(ttl_ms.to_string().into()),
            )
            .await
            .map_err(rabbitmq_error("publish"))?
            .await
            .map_err(rabbitmq_error("publish_confirm"))?;
        Ok(())
    }
}
