use crate::{
    tcp::types::{TcpRequest, TcpResponse},
    Broker,
};
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

pub async fn run_server<B>(addr: SocketAddr, broker: B) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let listener = TcpListener::bind(addr).await?;
    serve(listener, broker).await
}

pub async fn serve<B>(listener: TcpListener, broker: B) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let broker = Arc::new(broker);

    loop {
        let (stream, _) = listener.accept().await?;
        let broker = Arc::clone(&broker);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, broker).await {
                eprintln!("broker tcp connection error: {err}");
            }
        });
    }
}

async fn handle_connection<B>(stream: TcpStream, broker: Arc<B>) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let request = serde_json::from_str::<TcpRequest>(line.trim_end());
    let response = match request {
        Ok(TcpRequest::Publish { message }) => {
            broker.publish(message).await.map(|_| TcpResponse::Ok)
        }
        Ok(TcpRequest::Receive { consumer }) => broker
            .receive(&consumer)
            .await
            .map(|delivery| TcpResponse::Delivery { delivery }),
        Ok(TcpRequest::Ack {
            consumer,
            delivery_id,
        }) => broker
            .ack(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        Ok(TcpRequest::Nack {
            consumer,
            delivery_id,
        }) => broker
            .nack(&consumer, delivery_id)
            .await
            .map(|_| TcpResponse::Ok),
        Err(err) => Ok(TcpResponse::Error {
            message: err.to_string(),
        }),
    }
    .unwrap_or_else(|err| TcpResponse::Error {
        message: err.to_string(),
    });

    let mut stream = reader.into_inner();
    let payload = serde_json::to_string(&response)?;
    stream.write_all(payload.as_bytes()).await?;
    stream.write_all(b"\n").await
}
