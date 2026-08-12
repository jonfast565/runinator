//! covers the `wss://` capability of the ws client's connect path: that tls is actually compiled in,
//! and that a process-default rustls `CryptoProvider` exists before the first upgrade.
//!
//! these deliberately do not stand up a real tls server — no certificate chain is needed to reach
//! either failure. but they do need a *live* listener: `tokio-tungstenite` connects the tcp socket
//! first and only then wraps it, so the tls gate sits behind a successful connect and a dead port
//! would never reach it. binding a plaintext listener and pointing `wss://` at it gets us past the
//! connect and straight to the tls step, which is the only part under test.

use super::*;
use tokio_tungstenite::tungstenite::error::{Error as WsError, UrlError};

#[tokio::test]
async fn wss_urls_are_supported() {
    // plaintext on purpose: the handshake is expected to fail, just not for the reason below.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        // accept and drop, so the client's ClientHello is answered by a close rather than by
        // nothing at all — a listener that only binds leaves the handshake blocked indefinitely.
        let _ = listener.accept().await;
    });

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio_tungstenite::connect_async(format!("wss://{addr}/")),
    )
    .await;

    // the regression this exists for: `tokio-tungstenite`'s default features do not include tls, and
    // without it every `wss://` url — i.e. every deployment behind an ingress — is refused the moment
    // the socket is wrapped, leaving the agent to reconnect-loop forever against a reachable cluster.
    // that refusal is immediate and local, so it can only land in the `Ok(Err(..))` arm; timing out
    // means we got as far as actually speaking tls, which is the thing being asserted.
    let Ok(result) = outcome else {
        return;
    };
    let err = result
        .err()
        .expect("a tls handshake against a plaintext listener must fail");
    assert!(
        !matches!(err, WsError::Url(UrlError::TlsFeatureNotEnabled)),
        "wss:// support is not compiled in; check runinator-broker's tokio-tungstenite features"
    );
}

#[tokio::test]
async fn a_default_crypto_provider_is_installed() {
    // building a default rustls client config is what `connect_async` does for a `wss://` url, and it
    // panics rather than erroring when both `ring` and `aws-lc-rs` are linked with neither installed
    // as the process default. this test fails as a panic if `install_crypto_provider` stops running.
    imp::install_crypto_provider();
    assert!(
        rustls::crypto::CryptoProvider::get_default().is_some(),
        "no process-default CryptoProvider; a wss:// upgrade would panic"
    );
}
