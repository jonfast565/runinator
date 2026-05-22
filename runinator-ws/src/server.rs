use std::{net::SocketAddr, sync::Arc};

use log::info;
use runinator_database::{initialize_database, interfaces::DatabaseImpl};
use runinator_models::errors::SendableError;
use tokio::{
    net::TcpListener,
    sync::{Notify, broadcast},
};

use crate::events::AppEvent;
use crate::handlers::catalog::seed_builtin_catalog;
use crate::router::build_router;

pub async fn run_webserver<T: DatabaseImpl>(
    pool: Arc<T>,
    notify: Arc<Notify>,
    port: u16,
) -> Result<(), SendableError> {
    initialize_database(&pool).await?;
    seed_builtin_catalog(pool.as_ref()).await?;
    let (events_tx, _) = broadcast::channel::<AppEvent>(1024);
    let app = build_router(pool, events_tx);
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    let listener = TcpListener::bind(addr).await?;
    let server = axum::serve(listener, app);
    info!("Webserver started at {}:{}", addr.ip(), addr.port());

    tokio::select! {
        result = server => {
            if let Err(err) = result {
                log::error!("Webserver error: {}", err);
                return Err(Box::new(err));
            }
            Ok(())
        }
        _ = notify.notified() => {
            info!("Shutting down web server...");
            Ok(())
        }
    }
}
