//! Circuit transitions for the HTTP implementation.
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use super::*;

fn status_server(statuses: Vec<u16>) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let server_calls = calls.clone();
    let task = thread::spawn(move || {
        for status in statuses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            server_calls.fetch_add(1, Ordering::SeqCst);
            let reason = if status == 200 { "OK" } else { "Test Failure" };
            let body = if status == 200 { "{}" } else { "failure" };
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (format!("http://{address}"), calls, task)
}

#[test]
fn transient_failures_fast_fail_then_a_successful_probe_closes_the_adapter_circuit() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (base_url, calls, server) = status_server(vec![500, 500, 200]);
        let circuit = AdapterCircuit::new(true, 2, Duration::from_millis(1));
        let http = reqwest::Client::new();
        for _ in 0..2 {
            let response =
                send_with_circuit(&http, &circuit, http.get(format!("{base_url}/failing")))
                    .await
                    .unwrap();
            assert_eq!(
                response.status(),
                reqwest::StatusCode::INTERNAL_SERVER_ERROR
            );
        }
        let error = send_with_circuit(&http, &circuit, http.get(format!("{base_url}/skipped")))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            AdapterClientError::CircuitOpen {
                retry_after_seconds: 1
            }
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        tokio::time::sleep(Duration::from_millis(5)).await;
        let response = send_with_circuit(&http, &circuit, http.get(format!("{base_url}/probe")))
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        server.join().unwrap();
    });
}

#[test]
fn normal_4xx_responses_do_not_open_the_adapter_circuit() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (base_url, calls, server) = status_server(vec![400, 400, 400]);
        let circuit = AdapterCircuit::new(true, 2, Duration::from_secs(1));
        let http = reqwest::Client::new();
        for _ in 0..3 {
            let response = send_with_circuit(
                &http,
                &circuit,
                http.get(format!("{base_url}/client-error")),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        server.join().unwrap();
    });
}
