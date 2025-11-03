use std::{sync::Arc, time::Duration};

use clap::{Parser, ValueEnum};
use log::{error, info, warn};
use runinator_comm::{
    discovery::{broadcast_gossip_message, gossip_targets},
    GossipMessage, WebServiceAnnouncement,
};
use runinator_database::{initialize_database, postgres::PostgresDb, sqlite::SqliteDb};
use runinator_models::errors::SendableError;
use tokio::{net::UdpSocket, sync::Notify, time};
use uuid::Uuid;

use runinator_ws::run_webserver;

#[derive(Clone, Debug, ValueEnum)]
enum DatabaseKind {
    Sqlite,
    Postgres,
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    /// Webservice port to bind to, defaults to 8080
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Database backend to use
    #[arg(long, value_enum, default_value_t = DatabaseKind::Sqlite)]
    database: DatabaseKind,

    /// Path to the SQLite database file (used when --database=sqlite)
    #[arg(long, default_value = "runinator.db")]
    sqlite_path: String,

    /// Connection string for the database (required when --database=postgres)
    #[arg(long)]
    database_url: Option<String>,

    /// Address to bind the gossip socket for service discovery
    #[arg(long, default_value = "0.0.0.0")]
    gossip_bind: String,

    /// Gossip UDP port
    #[arg(long, default_value_t = 5000)]
    gossip_port: u16,

    /// Additional gossip targets as host:port, comma separated
    #[arg(long, value_delimiter = ',', default_value = "")]
    gossip_targets: Vec<String>,

    /// Address advertised to other services (e.g. public IP or pod IP)
    #[arg(long, default_value = "127.0.0.1")]
    announce_address: String,

    /// Base path advertised to other services
    #[arg(long, default_value = "/")]
    announce_base_path: String,

    /// Seconds between gossip announcements
    #[arg(long, default_value_t = 5)]
    gossip_interval_seconds: u64,
}

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    env_logger::init();
    let args = CliArgs::parse();

    let notify = Arc::new(Notify::new());
    let shutdown_listener = notify.clone();
    tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            log::error!("Failed to listen for shutdown signal: {}", err);
            return;
        }
        info!("Shutdown signal received, stopping web server...");
        shutdown_listener.notify_waiters();
    });

    let CliArgs {
        port,
        database,
        sqlite_path,
        database_url,
        gossip_bind,
        gossip_port,
        gossip_targets,
        announce_address,
        announce_base_path,
        gossip_interval_seconds,
    } = args;

    let service_id = Uuid::new_v4();
    spawn_gossip_advertiser(
        service_id,
        gossip_bind,
        gossip_port,
        gossip_targets,
        announce_address.clone(),
        announce_base_path.clone(),
        gossip_interval_seconds,
        notify.clone(),
        port,
    );

    match database {
        DatabaseKind::Sqlite => {
            info!(
                "Starting Runinator webservice with SQLite database at {}",
                sqlite_path
            );
            let db = Arc::new(SqliteDb::new(&sqlite_path).await?);
            initialize_database(&db).await?;
            run_webserver(db, notify.clone(), port).await?;
        }
        DatabaseKind::Postgres => {
            let url = database_url
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "--database-url must be provided when --database=postgres",
                    )
                })
                .map_err(|err| -> SendableError { Box::new(err) })?;

            info!("Starting Runinator webservice with Postgres database");
            let db = Arc::new(PostgresDb::new(&url).await?);
            initialize_database(&db).await?;
            run_webserver(db, notify.clone(), port).await?;
        }
    }

    Ok(())
}

fn spawn_gossip_advertiser(
    service_id: Uuid,
    bind_addr: String,
    gossip_port: u16,
    extra_targets: Vec<String>,
    announce_address: String,
    announce_base_path: String,
    interval_seconds: u64,
    notify: Arc<Notify>,
    service_port: u16,
) {
    tokio::spawn(async move {
        let socket = match UdpSocket::bind((bind_addr.as_str(), 0)).await {
            Ok(socket) => {
                if let Err(err) = socket.set_broadcast(true) {
                    warn!("Unable to enable broadcast on gossip socket: {}", err);
                }
                socket
            }
            Err(err) => {
                error!("Failed to bind gossip socket: {}", err);
                return;
            }
        };

        let targets = gossip_targets(gossip_port, extra_targets);

        let interval = Duration::from_secs(interval_seconds.max(1));
        let mut ticker = time::interval(interval);
        info!(
            "Advertising Runinator web service via gossip on UDP port {}",
            gossip_port
        );

        loop {
            tokio::select! {
                _ = notify.notified() => break,
                _ = ticker.tick() => {
                    let announcement = WebServiceAnnouncement {
                        service_id,
                        address: announce_address.clone(),
                        port: service_port,
                        base_path: Some(announce_base_path.clone()),
                        last_heartbeat: chrono::Utc::now(),
                    };

                    let message = GossipMessage::WebService {
                        service: announcement,
                    };
                    broadcast_gossip_message(&socket, &message, &targets).await;
                }
            }
        }
        info!("Stopped gossip advertisements");
    });
}
