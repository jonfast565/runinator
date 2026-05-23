use runinator_broker::{http, in_memory::InMemoryBroker, tcp};
use std::{env, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = env::var("RUNINATOR_BROKER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:7070".into())
        .parse()?;
    let transport = env::var("RUNINATOR_BROKER_TRANSPORT").unwrap_or_else(|_| "tcp".into());

    let broker = InMemoryBroker::new();
    match transport.as_str() {
        "tcp" => {
            println!("Runinator TCP broker listening on {}", addr);
            tcp::server::run_server(addr, broker).await?;
        }
        "http" => {
            println!("Runinator HTTP broker listening on {}", addr);
            http::server::run_server(addr, broker).await?;
        }
        other => {
            return Err(format!("Unknown broker transport '{other}'").into());
        }
    }
    Ok(())
}
