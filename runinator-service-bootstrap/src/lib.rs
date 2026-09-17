//! Composable infrastructure startup for Runinator service executables.
//!
//! This crate owns only shared process resources. Service runtimes, replica registration, and
//! route assembly remain in their domain crates.

use std::{error::Error, fmt, path::PathBuf, sync::Arc};

use runinator_blob::{BlobError, BlobStore};
use runinator_broker::{Broker, build_broker_client};
use runinator_db_cli::{DatabaseBackend, prepare_sqlite_path, required_database_url};
use runinator_platform::startup::ProcessResources;

pub use runinator_broker::{BrokerBuildError, BrokerClientConfig, BrokerConsumerProfile};
pub use runinator_db_cli;

/// A completed set of the infrastructure resources requested by a service.

/// Selects exactly the shared resources a server needs before creating them.

async fn resolve_database(
    request: DatabaseRequest,
) -> Result<DatabaseResource, ServerBootstrapError> {
    let sqlite_path = match request.sqlite_path {
        Some(path) => path,
        None => runinator_platform::app_data::default_sqlite_path()
            .map_err(ServerBootstrapError::Process)?,
    };
    let sqlite_connection = prepare_sqlite_path(sqlite_path)
        .await
        .map_err(|error| ServerBootstrapError::Database(Box::new(error)))?;
    let url = match request.backend {
        DatabaseBackend::Sqlite => String::new(),
        DatabaseBackend::Postgres | DatabaseBackend::Mariadb => {
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
    Blob(BlobError),
    Database(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for ServerBootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process(error) => write!(f, "process startup: {error}"),
            Self::Broker(error) => write!(f, "broker startup: {error}"),
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
    use super::*;

    fn unique_sqlite_path() -> PathBuf {
        let suffix = runinator_platform::time::unix_timestamp_nanos();
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
            .build()
            .await
            .unwrap();
        assert!(resources.broker().is_some());
        assert!(resources.blobs().is_none());
        assert!(resources.database().is_none());
    }
}

mod blob_request;
pub use blob_request::BlobRequest;

mod database_request;
pub use database_request::DatabaseRequest;

mod database_resource;
pub use database_resource::DatabaseResource;

mod server_resources;
pub use server_resources::ServerResources;

mod server_resources_builder;
pub use server_resources_builder::ServerResourcesBuilder;
