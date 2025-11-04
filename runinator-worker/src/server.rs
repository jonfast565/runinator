use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use crate::executor::execute_task;
use log::{error, info};
use runinator_comm::{TaskCommand, TaskResult};
use runinator_models::errors::{RuntimeError, SendableError};
use runinator_plugin::plugin::Plugin;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

pub async fn run_command_server(
    bind_address: &str,
    port: u16,
    libraries: Arc<HashMap<String, Plugin>>,
) -> Result<(), SendableError> {
    let listener = TcpListener::bind((bind_address, port)).await?;
    info!("Worker listening for commands on {}:{}", bind_address, port);

    loop {
        let (socket, peer_addr) = listener.accept().await?;
        let libs = Arc::clone(&libraries);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(socket, libs, peer_addr).await {
                error!("Error handling worker command: {}", err);
            }
        });
    }
}

async fn handle_connection(
    socket: TcpStream,
    libraries: Arc<HashMap<String, Plugin>>,
    peer_addr: SocketAddr,
) -> Result<(), SendableError> {
    let mut reader = BufReader::new(socket);
    let mut line = String::new();

    let bytes = reader
        .read_line(&mut line)
        .await
        .map_err(|err| -> SendableError { Box::new(err) })?;

    if bytes == 0 {
        return Err(Box::new(RuntimeError::new(
            "worker.connection.empty".into(),
            "No data received from scheduler".into(),
        )));
    }

    let line = line.trim();
    let command = TaskCommand::from_json(line).map_err(|err| -> SendableError { Box::new(err) })?;

    info!(
        "Received task {} from {}",
        command.task.id.unwrap_or_default(),
        peer_addr
    );

    let result = execute_task(libraries, command.command_id, command.task).await;
    send_response(reader, result).await
}

async fn send_response(
    reader: BufReader<TcpStream>,
    result: TaskResult,
) -> Result<(), SendableError> {
    let mut socket = reader.into_inner();
    let payload = result
        .to_json()
        .map_err(|err| -> SendableError { Box::new(err) })?;
    socket
        .write_all(payload.as_bytes())
        .await
        .map_err(|err| -> SendableError { Box::new(err) })?;
    socket
        .write_all(b"\n")
        .await
        .map_err(|err| -> SendableError { Box::new(err) })?;
    socket
        .flush()
        .await
        .map_err(|err| -> SendableError { Box::new(err) })?;
    Ok(())
}
