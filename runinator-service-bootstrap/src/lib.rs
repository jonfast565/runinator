//! Composable infrastructure startup for Runinator service executables.
//!
//! This crate owns only shared process resources. Service runtimes, replica registration, and
//! route assembly remain in their domain crates.

use std::{error::Error, fmt, path::PathBuf, sync::Arc};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_blob::{BlobError, BlobStore};
use runinator_broker::{Broker, build_broker_client};
use runinator_db_cli::{DatabaseBackend, prepare_sqlite_path, required_database_url};
use runinator_utilities::startup::ProcessResources;

pub use runinator_broker::{BrokerBuildError, BrokerClientConfig, BrokerConsumerProfile};
pub use runinator_db_cli;

/// Inputs for a static, authenticated web-service client.
#[derive(Debug, Clone)]
pub struct ApiClientConfig {
    pub base_url: String,
    pub api_key: Option<String>,
}

/// Request the process's object store and optionally reconcile its buckets on startup.
#[derive(Debug, Clone, Copy)]
pub struct BlobRequest {
    pub ensure_buckets: bool,
}

/// Explicit database selection parsed by a service's existing CLI.
#[derive(Debug, Clone)]
pub struct DatabaseRequest {
    pub backend: DatabaseBackend,
    pub sqlite_path: Option<PathBuf>,
    pub database_url: Option<String>,
}

/// Resolved database input suitable for concrete, macro-based database dispatch.
#[derive(Debug, Clone)]
pub struct DatabaseResource {
    backend: DatabaseBackend,
    sqlite_connection: String,
    url: String,
}

impl DatabaseResource {
    pub fn backend(&self) -> DatabaseBackend {
        self.backend
    }

    pub fn sqlite_connection(&self) -> &str {
        &self.sqlite_connection
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

/// A completed set of the infrastructure resources requested by a service.
pub struct ServerResources {
    process: ProcessResources,
    broker: Option<Arc<dyn Broker>>,
    api: Option<AsyncApiClient<StaticLocator>>,
    blobs: Option<Arc<dyn BlobStore>>,
    database: Option<DatabaseResource>,
}

impl ServerResources {
    pub fn builder(name: impl Into<String>) -> ServerResourcesBuilder {
        ServerResourcesBuilder::new(name)
    }

    pub fn process(&self) -> &ProcessResources {
        &self.process
    }

    pub fn broker(&self) -> Option<&Arc<dyn Broker>> {
        self.broker.as_ref()
    }

    pub fn api(&self) -> Option<&AsyncApiClient<StaticLocator>> {
        self.api.as_ref()
    }

    pub fn blobs(&self) -> Option<&Arc<dyn BlobStore>> {
        self.blobs.as_ref()
    }

    pub fn database(&self) -> Option<&DatabaseResource> {
        self.database.as_ref()
    }
}

/// Selects exactly the shared resources a server needs before creating them.
pub struct ServerResourcesBuilder {
    name: String,
    broker: Option<(BrokerClientConfig, BrokerConsumerProfile)>,
    api: Option<ApiClientConfig>,
    blobs: Option<BlobRequest>,
    database: Option<DatabaseRequest>,
}

impl ServerResourcesBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            broker: None,
            api: None,
            blobs: None,
            database: None,
        }
    }

    pub fn broker(mut self, config: BrokerClientConfig, profile: BrokerConsumerProfile) -> Self {
        self.broker = Some((config, profile));
        self
    }

    pub fn api(mut self, config: ApiClientConfig) -> Self {
        self.api = Some(config);
        self
    }

    pub fn blobs(mut self, request: BlobRequest) -> Self {
        self.blobs = Some(request);
        self
    }

    pub fn database(mut self, request: DatabaseRequest) -> Self {
        self.database = Some(request);
        self
    }

    pub async fn build(self) -> Result<ServerResources, ServerBootstrapError> {
        let process = ProcessResources::start(&self.name).map_err(ServerBootstrapError::Process)?;
        let broker = match self.broker {
            Some((config, profile)) => Some(
                build_broker_client(&config, profile)
                    .await
                    .map_err(ServerBootstrapError::Broker)?,
            ),
            None => None,
        };
        let api = match self.api {
            Some(config) => Some(
                AsyncApiClient::with_credentials(
                    StaticLocator::new(config.base_url),
                    config.api_key,
                )
                .map_err(ServerBootstrapError::Api)?,
            ),
            None => None,
        };
        let blobs = match self.blobs {
            Some(request) => {
                let store = runinator_blob::from_env()
                    .await
                    .map_err(ServerBootstrapError::Blob)?;
                if request.ensure_buckets {
                    runinator_blob::ensure_buckets(&store)
                        .await
                        .map_err(ServerBootstrapError::Blob)?;
                }
                Some(store)
            }
            None => None,
        };
        let database = match self.database {
            Some(request) => Some(resolve_database(request).await?),
            None => None,
        };
        Ok(ServerResources {
            process,
            broker,
            api,
            blobs,
            database,
        })
    }
}

async fn resolve_database(
    request: DatabaseRequest,
) -> Result<DatabaseResource, ServerBootstrapError> {
    let sqlite_path = match request.sqlite_path {
        Some(path) => path,
        None => runinator_utilities::app_data::default_sqlite_path()
            .map_err(ServerBootstrapError::Process)?,
    };
    let sqlite_connection = prepare_sqlite_path(sqlite_path)
        .await
        .map_err(|error| ServerBootstrapError::Database(Box::new(error)))?;
    let url = match request.backend {
        DatabaseBackend::Sqlite => String::new(),
        DatabaseBackend::Postgres | DatabaseBackend::Mysql => {
            required_database_url(request.database_url).map_err(ServerBootstrapError::Database)?
        }
    };
    Ok(DatabaseResource {
        backend: request.backend,
        sqlite_connection,
        url,
    })
}

/// Typed failure from shared resource construction. Executables map variants to their established
/// error dictionaries at their outer boundary.
#[derive(Debug)]
pub enum ServerBootstrapError {
    Process(Box<dyn Error + Send + Sync>),
    Broker(BrokerBuildError),
    Api(reqwest::Error),
    Blob(BlobError),
    Database(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for ServerBootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process(error) => write!(f, "process startup: {error}"),
            Self::Broker(error) => write!(f, "broker startup: {error}"),
            Self::Api(error) => write!(f, "api client startup: {error}"),
            Self::Blob(error) => write!(f, "blob store startup: {error}"),
            Self::Database(error) => write!(f, "database startup: {error}"),
        }
    }
}

impl Error for ServerBootstrapError {}

/// Dispatch a resolved [`DatabaseResource`] to the concrete database selected by the service CLI.
#[macro_export]
macro_rules! dispatch_server_database {
    ($resource:expr, |$db:ident| $body:block) => {{
        let __resource = &$resource;
        $crate::runinator_db_cli::dispatch_database!(
            __resource.backend(),
            sqlite: __resource.sqlite_connection().to_string(),
            url: __resource.url().to_string(),
            |$db| $body
        )
    }};
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn unique_sqlite_path() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("runinator-service-bootstrap-{suffix}"))
            .join("runinator.db")
    }

    #[tokio::test]
    async fn resolves_sqlite_database_and_creates_its_parent() {
        let path = unique_sqlite_path();
        let database = resolve_database(DatabaseRequest {
            backend: DatabaseBackend::Sqlite,
            sqlite_path: Some(path.clone()),
            database_url: None,
        })
        .await
        .unwrap();
        assert_eq!(database.backend().label(), "sqlite");
        assert_eq!(database.sqlite_connection(), path.to_string_lossy());
        assert!(path.parent().unwrap().exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn builds_only_requested_process_resources() {
        let resources = ServerResources::builder("Bootstrap test")
            .broker(
                BrokerClientConfig {
                    backend: "in-memory".into(),
                    endpoint: String::new(),
                    control_topic: "control".into(),
                    agent_topic: None,
                    effect_topic: "effects".into(),
                    infrastructure_effect_topic: "effects.infrastructure".into(),
                    effect_result_topic: "effect-results".into(),
                    client_id: "test".into(),
                    relay_credential: None,
                    wake_topic: None,
                    ingress_topic: None,
                },
                BrokerConsumerProfile::Worker,
            )
            .api(ApiClientConfig {
                base_url: "http://127.0.0.1:8080/".into(),
                api_key: Some("test-key".into()),
            })
            .build()
            .await
            .unwrap();
        assert!(resources.broker().is_some());
        assert!(resources.api().is_some());
        assert!(resources.blobs().is_none());
        assert!(resources.database().is_none());
    }
}
