use log::info;
use runinator_config::parse_config;
use runinator_database::sqlite::SqliteDb;
use runinator_scheduler::scheduler_loop;
use runinator_ws::run_webserver;
use tokio::sync::Notify;
use std::{env, sync::Arc, time::SystemTime};

fn setup_logger() -> Result<(), Box<dyn std::error::Error>> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                humantime::format_rfc3339_seconds(SystemTime::now()),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .chain(fern::log_file("output.log")?)
        .apply()?;
    Ok(())
}

fn print_env() -> std::io::Result<()> {
    let path = env::current_dir()?;
    info!("The current directory is {}", path.display());
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env::set_var("RUST_BACKTRACE", "1");
    setup_logger()?;

    info!("--- Runinator ---");
    info!("--- Version 1 ---");
    print_env()?;

    info!("Parse config");
    let config = parse_config()?;

    // Initialize the SQLite connection pool
    info!("Initialize database pool");
    let pool = Arc::new(SqliteDb::new(&config.database).await?);
    let notify = Arc::new(Notify::new());

    // Start the scheduler in a separate task
    info!("Initialize scheduler");
    let notify_scheduler = notify.clone();
    let scheduler_config = (&config).clone();
    let scheduler_pool = pool.clone();
    let scheduler_task = tokio::spawn(async move {
        scheduler_loop(&scheduler_pool, notify_scheduler, &scheduler_config).await.expect("scheduler does not fail");
    });

    // Start the web server in a separate task
    info!("Initialize web server");
    let ws_config = (&config).clone();
    let ws_notify = notify.clone();
    let web_server_task = tokio::spawn(async move {
        run_webserver(&pool.clone(), ws_notify, ws_config.port).await;
    });

    info!("Initialization complete!");
    
    // Handle termination signals for graceful shutdown
    tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    info!("Received shutdown signal. Shutting down...");
    notify.notify_waiters();

    // Wait for the tasks to complete
    if let Err(e) = tokio::try_join!(scheduler_task, web_server_task) {
        log::error!("Error while shutting down: {:?}", e);
    }

    info!("Application shutdown complete.");
    Ok(())
}
