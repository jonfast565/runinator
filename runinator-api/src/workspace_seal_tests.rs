//! Workspace seal requests use the caller's remaining budget, including response receipt reads.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

#[test]
fn seal_overrides_generic_timeout_but_enforces_its_own_budget() {
    for budget in [Duration::from_millis(10), Duration::from_secs(2)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0; 8192];
            let read = stream.read(&mut bytes).unwrap();
            assert!(String::from_utf8_lossy(&bytes[..read]).contains("/seal?replica_id="));
            thread::sleep(Duration::from_millis(80));
            let _ = stream.write_all(
                b"HTTP/1.1 409 Conflict\r\nContent-Length: 8\r\nConnection: close\r\n\r\nconflict",
            );
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async {
            let client = Client::builder()
                .timeout(Duration::from_millis(5))
                .build()
                .unwrap();
            let api = AsyncApiClient::with_client(crate::StaticLocator::new(url), client);
            api.seal_workspace(Uuid::new_v4(), Uuid::new_v4(), "revision".into(), budget)
                .await
        });
        if budget < Duration::from_millis(80) {
            assert!(matches!(result, Err(ApiError::Request(error)) if error.is_timeout()));
        } else {
            assert!(
                matches!(result, Err(ApiError::Http { status, .. }) if status == reqwest::StatusCode::CONFLICT)
            );
        }
        server.join().unwrap();
    }
}
