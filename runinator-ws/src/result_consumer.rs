use std::sync::Arc;

use log::{error, info};
use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use tokio::sync::Notify;

use crate::{
    events::{EventSender, emit_workflow_node_run},
    repository,
};

const RESULT_CONSUMER_ID: &str = "runinator-ws-results";

pub(crate) async fn run_result_consumer<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    events: EventSender,
    shutdown: Arc<Notify>,
) {
    info!("Workflow result consumer started");
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => {
                info!("Workflow result consumer shutting down");
                return;
            }
            result = broker.receive_result(RESULT_CONSUMER_ID) => {
                match result {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        error!("Failed to receive workflow result event: {}", err);
                        continue;
                    }
                }
            }
        };

        let node_run_id = delivery.event.workflow_node_run_id;
        match repository::apply_workflow_result_event(db.as_ref(), &delivery.event).await {
            Ok(_) => {
                emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
                if let Err(err) = broker
                    .ack_result(RESULT_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    error!(
                        "Failed to ack workflow result event {}: {}",
                        delivery.event.event_id, err
                    );
                }
            }
            Err(err) => {
                error!(
                    "Failed to persist workflow result event {}: {}",
                    delivery.event.event_id, err
                );
                if let Err(err) = broker
                    .nack_result(RESULT_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    error!(
                        "Failed to nack workflow result event {}: {}",
                        delivery.event.event_id, err
                    );
                }
            }
        }
    }
}
