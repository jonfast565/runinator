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
            schema_digest: None,
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

// a first poll over a busy source walks every item, so an operator needs a way to buy it more time
// than the incremental polls that follow ever use.
#[test]
fn an_adapter_without_configuration_keeps_the_default_timeout() {
    let configuration = runinator_models::value::Value::from(serde_json::json!({}));
    assert_eq!(timeout_seconds(&configuration), DEFAULT_TIMEOUT_SECONDS);
}

#[test]
fn a_configured_timeout_is_honoured() {
    let configuration =
        runinator_models::value::Value::from(serde_json::json!({"poll_timeout_seconds": 600}));
    assert_eq!(timeout_seconds(&configuration), 600);
}

#[test]
fn a_timeout_beyond_the_ceiling_is_clamped() {
    let configuration =
        runinator_models::value::Value::from(serde_json::json!({"poll_timeout_seconds": 86_400}));
    assert_eq!(timeout_seconds(&configuration), MAX_TIMEOUT_SECONDS);
}

// zero or a negative would otherwise dispatch work that can never finish.
#[test]
fn a_nonsense_timeout_is_clamped_to_the_floor() {
    let configuration =
        runinator_models::value::Value::from(serde_json::json!({"poll_timeout_seconds": 0}));
    assert_eq!(timeout_seconds(&configuration), MIN_TIMEOUT_SECONDS);
}

// the interval and the timeout are independent knobs; setting one must not move the other.
#[test]
fn the_interval_is_unaffected_by_the_timeout() {
    let configuration =
        runinator_models::value::Value::from(serde_json::json!({"poll_timeout_seconds": 600}));
    assert_eq!(interval_seconds(&configuration), DEFAULT_RETRY_SECONDS);
}
