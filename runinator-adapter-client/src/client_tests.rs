//! HTTP client clones share a circuit without sharing state across independent clients.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

#[test]
fn clones_share_circuit_state_but_independent_clients_do_not() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for status in [500, 200] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            let size = socket.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..size]).to_ascii_lowercase();
            assert!(request.starts_with("get /health "));
            assert!(request.contains("authorization: bearer test-token"));
            write!(
                socket,
                "HTTP/1.1 {status} Test\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            )
            .unwrap();
        }
    });
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut first = HttpAdapterHostClient::new(url.clone(), Some("test-token".into()));
            first.circuit = AdapterCircuit::new(true, 1, Duration::from_secs(60));
            let clone = first.clone();
            assert!(matches!(
                first.health().await,
                Err(AdapterClientError::Http { .. })
            ));
            assert!(matches!(
                clone.health().await,
                Err(AdapterClientError::CircuitOpen { .. })
            ));
            let second = HttpAdapterHostClient::new(url, Some("test-token".into()));
            assert_eq!(second.health().await.unwrap(), serde_json::json!({}));
        });
    server.join().unwrap();
}
