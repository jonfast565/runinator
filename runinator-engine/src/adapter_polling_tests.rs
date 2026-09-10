//! Polling failures through the injected adapter host preserve durable accounting and cooldowns.
use super::*;
use runinator_adapter_client::{AdapterClientError, AdapterPoller};
use runinator_broker_core::{UiEventPublisher, in_memory::InMemoryBroker};
use runinator_models::orchestration::AdapterTransport;
use runinator_store::{
    DatabaseImpl,
    roles::{NewAdapterDefinition, OrchestrationStore},
};

struct UnavailableHost;
#[async_trait::async_trait]
impl AdapterPoller for UnavailableHost {
    async fn poll(
        &self,
        kind: &str,
        request: AdapterPollRequest,
    ) -> runinator_adapter_client::Result<AdapterPollResponse> {
        assert_eq!(kind, "github");
        assert!(request.initialize);
        Err(AdapterClientError::CircuitOpen {
            retry_after_seconds: 123,
        })
    }
}

#[tokio::test]
async fn injected_circuit_failure_keeps_its_cooldown() {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(
        runinator_database::sqlite::SqliteDb::new(
            directory.path().join("poll.db").to_str().unwrap(),
        )
        .await
        .unwrap(),
    );
    db.run_init_scripts(&Vec::new()).await.unwrap();
    db.create_orchestration_adapter(
        NewAdapterDefinition {
            id: uuid::Uuid::now_v7(),
            org_id: uuid::Uuid::now_v7(),
            name: "injected poll".into(),
            kind: "github".into(),
            kind_version: "1".into(),
            transport: AdapterTransport::Polling,
            endpoint_identity: uuid::Uuid::now_v7().to_string(),
            configuration: Value::Null,
            authentication: Default::default(),
            identity_configuration: Value::Null,
            actor_id: None,
        },
        Utc::now(),
    )
    .await
    .unwrap();
    let claims = db
        .claim_due_orchestration_adapter_polls(
            "test".into(),
            Utc::now(),
            Utc::now() + TimeDelta::seconds(300),
            1,
        )
        .await
        .unwrap();
    let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
    let pipelines = PipelineOperations::new(
        db.clone(),
        broker.clone(),
        UiEventPublisher::new(broker),
        None,
    );
    let failure = poll_one(
        db,
        &UnavailableHost,
        &pipelines,
        "test",
        claims.into_iter().next().unwrap(),
    )
    .await
    .err()
    .expect("injected failure");
    assert_eq!(failure.retry_after_seconds, 123);
    assert_eq!(failure.message, "adapter-host circuit is open");
}
