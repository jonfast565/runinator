#[cfg(feature = "http")]
use runinator_broker::http;
use runinator_broker::in_memory::InMemoryBroker;
use runinator_broker::tcp;
use runinator_utilities::startup::ProcessResources;
use std::{env, net::SocketAddr};

#[path = "main/service.rs"]
mod service;
use service::BrokerService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    BrokerService::new().run().await
}

async fn run_process() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let process = ProcessResources::start("Runinator Broker")?;
    let shutdown = process.shutdown();
    let addr: SocketAddr = env::var("RUNINATOR_BROKER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:7070".into())
        .parse()?;
    let transport = env::var("RUNINATOR_BROKER_TRANSPORT").unwrap_or_else(|_| "tcp".into());

    let broker = InMemoryBroker::new();
    match transport.as_str() {
        "tcp" => {
            println!("Runinator TCP broker listening on {}", addr);
            tokio::select! {
                result = tcp::server::run_server(addr, broker) => result?,
                _ = shutdown.cancelled() => eprintln!("broker shutdown signal received"),
            }
        }
        #[cfg(feature = "http")]
        "http" => {
            println!("Runinator HTTP broker listening on {}", addr);
            tokio::select! {
                result = http::server::run_server(addr, broker) => result?,
                _ = shutdown.cancelled() => eprintln!("broker shutdown signal received"),
            }
        }
        other => {
            return Err(format!("Unknown broker transport '{other}'").into());
        }
    }
    Ok(())
}
