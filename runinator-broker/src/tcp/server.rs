use crate::{dispatch::dispatch, tcp::types::TcpRequest, Broker};
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
            let Err(err) = handle_connection(stream, broker).await else {
                return;
            };
            if is_client_disconnect(&err) {
                return;
            }
            eprintln!("broker tcp connection error: {err}");
        });
    }
}

fn is_client_disconnect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::UnexpectedEof
    )
}

async fn handle_connection<B>(stream: TcpStream, broker: Arc<B>) -> Result<(), std::io::Error>
where
    B: Broker,
{
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let response = match serde_json::from_str::<TcpRequest>(line.trim_end()) {
        Ok(request) => dispatch(broker.as_ref(), request).await,
        Err(err) => crate::tcp::types::TcpResponse::Error {
            message: err.to_string(),
        },
    };

    let mut stream = reader.into_inner();
    let payload = serde_json::to_string(&response)?;
    stream.write_all(payload.as_bytes()).await?;
    stream.write_all(b"\n").await
}
