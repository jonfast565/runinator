#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct Config {
    pub database: DatabaseBackend,
    pub database_url: String,
    pub archive_dir: PathBuf,
    pub interval: Duration,
    pub claim_lease: Duration,
    pub batch_size: i64,
    pub dry_run: bool,
    pub workflow_run_retention: Option<Duration>,
    pub pipeline_run_retention: Option<Duration>,
    pub effect_dispatch_retention: Option<Duration>,
    pub read_notification_retention: Option<Duration>,
    pub dead_letter_retention: Option<Duration>,
    pub audit_log_retention: Option<Duration>,
    pub idempotency_retention: Option<Duration>,
    pub automation_retention: Option<Duration>,
    pub usage_retention: Option<Duration>,
    pub revision_retention: Option<Duration>,
    pub agent_directive_retention: Option<Duration>,
    pub archive_ledger_retention: Option<Duration>,
    pub security_retention: Option<Duration>,
    pub cooldown_retention: Option<Duration>,
    pub liveness_file: String,
    pub broker_backend: String,
    pub broker_endpoint: String,
    pub broker_mode: String,
    pub service_url: Option<String>,
    pub api_key: Option<String>,
    pub broker_relay_path: String,
    pub broker_effect_topic: String,
    pub broker_infrastructure_effect_topic: String,
    pub broker_control_topic: String,
    pub broker_effect_result_topic: String,
    pub broker_wake_topic: String,
    pub broker_ingress_topic: String,
    pub broker_client_id: String,
    pub advertise_host: Option<String>,
}

impl Config {
    pub fn from_cli(cli: Cli) -> Result<Self, SendableError> {
        let database_url = cli
            .database_url
            .or_else(|| runinator_platform::env::string("DATABASE_URL"))
            .ok_or_else(|| -> SendableError {
                "missing connection string: pass --database-url or set RUNINATOR_DATABASE_URL"
                    .into()
            })?;
        Ok(Self {
            database: cli.database,
            database_url,
            archive_dir: cli.archive_dir,
            interval: parse_required_duration(&cli.interval)?,
            claim_lease: parse_required_duration(&cli.claim_lease)?,
            batch_size: cli.batch_size.max(1),
            dry_run: cli.dry_run,
            workflow_run_retention: parse_optional_duration(&cli.workflow_run_retention)?,
            pipeline_run_retention: parse_optional_duration(&cli.pipeline_run_retention)?,
            effect_dispatch_retention: parse_optional_duration(&cli.effect_dispatch_retention)?,
            read_notification_retention: parse_optional_duration(&cli.read_notification_retention)?,
            dead_letter_retention: parse_optional_duration(&cli.dead_letter_retention)?,
            audit_log_retention: parse_optional_duration(&cli.audit_log_retention)?,
            idempotency_retention: parse_optional_duration(&cli.idempotency_retention)?,
            automation_retention: parse_optional_duration(&cli.automation_retention)?,
            usage_retention: parse_optional_duration(&cli.usage_retention)?,
            revision_retention: parse_optional_duration(&cli.revision_retention)?,
            agent_directive_retention: parse_optional_duration(&cli.agent_directive_retention)?,
            archive_ledger_retention: parse_optional_duration(&cli.archive_ledger_retention)?,
            security_retention: parse_optional_duration(&cli.security_retention)?,
            cooldown_retention: parse_optional_duration(&cli.cooldown_retention)?,
            liveness_file: cli.liveness_file,
            broker_backend: cli.broker_backend,
            broker_endpoint: cli.broker_endpoint,
            broker_mode: cli.broker_mode,
            service_url: cli.service_url,
            api_key: cli.api_key,
            broker_relay_path: cli.broker_relay_path,
            broker_effect_topic: cli.broker_effect_topic,
            broker_infrastructure_effect_topic: cli.broker_infrastructure_effect_topic,
            broker_control_topic: cli.broker_control_topic,
            broker_effect_result_topic: cli.broker_effect_result_topic,
            broker_wake_topic: cli.broker_wake_topic,
            broker_ingress_topic: cli.broker_ingress_topic,
            broker_client_id: cli.broker_client_id,
            advertise_host: cli.advertise_host.filter(|value| !value.trim().is_empty()),
        })
    }

    /// Convert the legacy process flags into the operating policy used until the first persisted
    /// server policy exists. This preserves existing deployments during the settings migration.
    pub fn bootstrap_archiver_settings(&self) -> ArchiverSettings {
        ArchiverSettings {
            interval_seconds: self.interval.as_secs(),
            claim_lease_seconds: self.claim_lease.as_secs(),
            batch_size: self.batch_size.max(1) as u64,
            dry_run: self.dry_run,
            workflow_run_retention_seconds: seconds_or_disabled(self.workflow_run_retention),
            pipeline_run_retention_seconds: seconds_or_disabled(self.pipeline_run_retention),
            orchestration_retention_seconds: seconds_or_disabled(self.pipeline_run_retention),
            effect_dispatch_retention_seconds: seconds_or_disabled(self.effect_dispatch_retention),
            notification_retention_seconds: seconds_or_disabled(self.read_notification_retention),
            dead_letter_retention_seconds: seconds_or_disabled(self.dead_letter_retention),
            audit_log_retention_seconds: seconds_or_disabled(self.audit_log_retention),
            idempotency_retention_seconds: seconds_or_disabled(self.idempotency_retention),
            automation_retention_seconds: seconds_or_disabled(self.automation_retention),
            usage_retention_seconds: seconds_or_disabled(self.usage_retention),
            revision_retention_seconds: seconds_or_disabled(self.revision_retention),
            agent_directive_retention_seconds: seconds_or_disabled(self.agent_directive_retention),
            archive_ledger_retention_seconds: seconds_or_disabled(self.archive_ledger_retention),
            security_retention_seconds: seconds_or_disabled(self.security_retention),
            coordination_retention_seconds: seconds_or_disabled(self.cooldown_retention),
        }
    }
}
