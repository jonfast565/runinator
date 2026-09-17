#[allow(unused_imports)]
use super::*;

pub struct AgentRuntime;

impl AgentRuntime {
    /// start the lifecycle and return immediately. must be called from within a tokio runtime
    /// context.
    ///
    /// only configuration that cannot be retried fails here (an unusable service URL, an unreadable
    /// plugin path). anything the agent could recover from — the service being down, the broker
    /// being unreachable — is retried inside the lifecycle and reported through `observer`, so a
    /// host never has to implement its own retry policy to be robust.
    pub fn start(
        config: AgentRuntimeConfig,
        observer: Arc<dyn AgentObserver>,
    ) -> Result<AgentHandle, SendableError> {
        let api_client = AsyncApiClient::with_credentials(
            StaticLocator::new(config.service_url.clone()),
            config.api_key.clone(),
        )
        .map_err(|err| crate::errors::API_CLIENT.error(err))?;
        let libraries = Arc::new(load_libraries(&config.dll_paths)?);
        let result_outbox: Arc<dyn ResultOutbox> = Arc::new(
            FileOutbox::open(&config.outbox_file)
                .map_err(|err| crate::errors::API_CLIENT.error(err))?,
        );

        let report_context = Arc::new(AgentReportContext::new(
            &config,
            (config.providers)().len(),
            Arc::clone(&result_outbox),
        ));
        let reporter = Arc::new(StatusReporter::new(
            observer,
            AgentStatus {
                running: false,
                replica_id: None,
                connection: AgentConnection::Registering,
                broker_connection: Some(config.broker_description.clone()),
                metrics: Default::default(),
                last_error: None,
                last_error_at: None,
            },
        ));
        let telemetry = config
            .sample_telemetry
            .then(|| Arc::new(TelemetryCollector::new()));
        let shutdown = Shutdown::new();
        let state = reporter.subscribe();

        let task = tokio::spawn(run_lifecycle(AgentLifecycle {
            config,
            api_client,
            libraries,
            telemetry: telemetry.clone(),
            report_context,
            result_outbox,
            reporter: Arc::clone(&reporter),
            shutdown: shutdown.clone(),
        }));

        Ok(AgentHandle {
            shutdown,
            task,
            state,
            telemetry,
        })
    }
}
