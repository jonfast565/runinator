use runinator_broker::{in_memory::InMemoryBroker, tcp::server::run_server};
use std::{env, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = env::var("RUNINATOR_BROKER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:7070".into())
        .parse()?;

    let broker = InMemoryBroker::new();
    println!("Runinator TCP broker listening on {}", addr);
    run_server(addr, broker).await?;
    Ok(())
}
