use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde_json::Value;
use socket2::{Domain, Protocol, Socket, Type};
use tauri::{AppHandle, Emitter};
use url::Url;

use crate::{
    state::CommandCenterState,
    types::{ServiceStatus, WebServiceAnnouncement},
};

const DIRECT_SERVICE_URL_ENV: &[&str] = &[
    "RUNINATOR_COMMAND_CENTER_SERVICE_URL",
    "RUNINATOR_SERVICE_URL",
    "RUNINATOR_WS_SERVICE_URL",
    "WS_API_BASE_URL",
];

pub fn start_discovery_thread(app: AppHandle, state: CommandCenterState) {
    if state.mark_discovery_started() {
        println!("Discovery already started, ignoring redundant request.");
        return;
    }

    println!("Starting discovery thread...");
    match configured_service_url_from_env() {
        Ok(Some(url)) => {
            println!("Found configured service URL: {}", url);
            publish_service_url(&app, &state, url);
            return;
        }
        Err(err) => {
            eprintln!("Error checking configured service URL: {}", err);
            let _ = app.emit("service-discovery-error", err);
            return;
        }
        Ok(None) => {
            println!("No service URL configured in environment.");
        }
    }

    thread::spawn(move || {
        if let Err(err) = run_discovery_loop(app.clone(), state) {
            let _ = app.emit("service-discovery-error", err);
        }
    });
}

fn run_discovery_loop(app: AppHandle, state: CommandCenterState) -> Result<(), String> {
    println!("Gossip discovery loop started...");
    let bind_address = std::env::var("RUNINATOR_GOSSIP_BIND")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "0.0.0.0".to_string());
    let port = std::env::var("RUNINATOR_GOSSIP_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(5000);
    let ip = bind_address
        .parse::<IpAddr>()
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let socket = bind_discovery_socket(SocketAddr::new(ip, port))
        .map_err(|err| format!("Failed to bind gossip socket: {err}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|err| format!("Failed to configure gossip socket: {err}"))?;

    let mut services = HashMap::<String, WebServiceAnnouncement>::new();
    let mut buffer = [0_u8; 8192];
    let mut last_announced = Instant::now();
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((len, sender)) => {
                if let Some(service) = parse_announcement(&buffer[..len], sender.ip()) {
                    services.insert(service.service_id.clone(), service);
                    publish_best_service(&app, &state, &services);
                    last_announced = Instant::now();
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if services.is_empty() && last_announced.elapsed() > Duration::from_secs(30) {
                    println!("No gossip announcements received for 30s, still waiting...");
                    last_announced = Instant::now();
                }
            }
            Err(err) => return Err(format!("Failed to read gossip datagram: {err}")),
        }
    }
}

fn configured_service_url_from_env() -> Result<Option<String>, String> {
    for name in DIRECT_SERVICE_URL_ENV {
        if let Ok(value) = std::env::var(name) {
            println!("Checking env var {}: {}", name, value);
            if let Some(url) = configured_service_url_from_pairs(vec![(name.to_string(), value)])? {
                return Ok(Some(url));
            }
        }
    }

    if let Ok(host) = std::env::var("RUNINATOR_WS_SERVICE_HOST") {
        println!("Checking RUNINATOR_WS_SERVICE_HOST: {}", host);
        let port =
            std::env::var("RUNINATOR_WS_SERVICE_PORT").unwrap_or_else(|_| "8080".to_string());
        let scheme =
            std::env::var("RUNINATOR_WS_SERVICE_SCHEME").unwrap_or_else(|_| "http".to_string());
        let mut url_str = format!("{scheme}://{host}");
        if !host.contains(':') && port != "80" && port != "443" {
            url_str.push(':');
            url_str.push_str(&port);
        }
        return normalize_configured_service_url(&url_str)
            .map(Some)
            .map_err(|err| {
                let err_msg = format!("Invalid RUNINATOR_WS_SERVICE_HOST: {err}");
                eprintln!("{}", err_msg);
                err_msg
            });
    }

    // Default for local development if nothing else is found
    if std::env::var("TAURI_DEV").is_ok() {
        println!("TAURI_DEV detected, falling back to http://127.0.0.1:8080/");
        return Ok(Some("http://127.0.0.1:8080/".to_string()));
    }

    if std::env::var("CARGO_MANIFEST_DIR").is_ok() {
        println!("CARGO_MANIFEST_DIR detected, assuming local development and falling back to http://127.0.0.1:8080/");
        return Ok(Some("http://127.0.0.1:8080/".to_string()));
    }

    Ok(None)
}

fn configured_service_url_from_pairs<I>(pairs: I) -> Result<Option<String>, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    for (name, value) in pairs {
        if value.trim().is_empty() {
            continue;
        }
        return normalize_configured_service_url(&value)
            .map(Some)
            .map_err(|err| format!("Invalid {name}: {err}"));
    }
    Ok(None)
}

fn normalize_configured_service_url(value: &str) -> Result<String, String> {
    println!("Normalizing URL: {}", value);
    let mut url = Url::parse(value.trim()).map_err(|err| {
        let err_msg = format!("URL parse error for '{}': {}", value, err);
        eprintln!("{}", err_msg);
        err_msg
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("service URL must use http or https".into());
    }
    if url.host_str().is_none() {
        return Err("service URL must include a host".into());
    }
    url.set_query(None);
    url.set_fragment(None);

    let path = url.path().trim_end_matches('/');
    if path.is_empty() {
        url.set_path("/");
    } else {
        url.set_path(&format!("{path}/"));
    }
    Ok(url.to_string())
}

fn publish_best_service(
    app: &AppHandle,
    state: &CommandCenterState,
    services: &HashMap<String, WebServiceAnnouncement>,
) {
    if let Some(best) = services.values().max_by_key(|svc| svc.last_heartbeat) {
        publish_service_url(app, state, build_service_base_url(best));
    }
}

fn publish_service_url(app: &AppHandle, state: &CommandCenterState, url: String) {
    let service_url = state.service_url.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut current = service_url.write().await;
        if current.as_deref() == Some(url.as_str()) {
            println!("Service URL already set to {}, skipping publish", url);
            return;
        }
        println!("Publishing service URL: {}", url);
        *current = Some(url.clone());
        let _ = app.emit(
            "service-url-changed",
            ServiceStatus {
                service_url: Some(url),
            },
        );
    });
}

fn bind_discovery_socket(address: SocketAddr) -> std::io::Result<UdpSocket> {
    let domain = if address.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;

    if address.ip().is_multicast() {
        match address.ip() {
            IpAddr::V4(addr) => {
                socket
                    .join_multicast_v4(&addr, &Ipv4Addr::UNSPECIFIED)
                    .map_err(|e| {
                        eprintln!("Failed to join multicast v4 group {}: {}", addr, e);
                        e
                    })?;
                println!("Joined multicast v4 group: {}", addr);
            }
            IpAddr::V6(addr) => {
                socket.join_multicast_v6(&addr, 0).map_err(|e| {
                    eprintln!("Failed to join multicast v6 group {}: {}", addr, e);
                    e
                })?;
                println!("Joined multicast v6 group: {}", addr);
            }
        }
    }

    socket.bind(&address.into())?;
    Ok(socket.into())
}

fn parse_announcement(bytes: &[u8], sender: IpAddr) -> Option<WebServiceAnnouncement> {
    let root = serde_json::from_slice::<Value>(bytes).ok()?;
    if root.get("type").and_then(Value::as_str)? != "web_service" {
        return None;
    }
    let service = root.get("service")?.as_object()?;
    let address = service
        .get("address")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| sender.to_string());
    let port = service.get("port").and_then(Value::as_u64).unwrap_or(0) as u16;
    if port == 0 {
        return None;
    }
    let service_id = service
        .get("service_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{address}:{port}"));
    let base_path = service
        .get("base_path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let last_heartbeat = service
        .get("last_heartbeat")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .unwrap_or_else(Utc::now);
    Some(WebServiceAnnouncement {
        service_id,
        address,
        port,
        base_path,
        last_heartbeat,
    })
}

fn build_service_base_url(service: &WebServiceAnnouncement) -> String {
    let mut base = format!("http://{}:{}", service.address, service.port);
    let trimmed = service.base_path.trim();
    if !trimmed.is_empty() {
        if trimmed.starts_with('/') {
            base.push_str(trimmed);
        } else {
            base.push('/');
            base.push_str(trimmed);
        }
    }
    if !base.ends_with('/') {
        base.push('/');
    }
    base
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
