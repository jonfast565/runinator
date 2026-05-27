use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    thread,
    time::Duration,
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
    "WS_API_BASE_URL",
];

pub fn start_discovery_thread(app: AppHandle, state: CommandCenterState) {
    match configured_service_url_from_env() {
        Ok(Some(url)) => {
            publish_service_url(&app, &state, url);
            return;
        }
        Err(err) => {
            let _ = app.emit("service-discovery-error", err);
            return;
        }
        Ok(None) => {}
    }

    if state.mark_discovery_started() {
        return;
    }
    thread::spawn(move || {
        if let Err(err) = run_discovery_loop(app.clone(), state) {
            let _ = app.emit("service-discovery-error", err);
        }
    });
}

fn run_discovery_loop(app: AppHandle, state: CommandCenterState) -> Result<(), String> {
    let bind_address = std::env::var("RUNINATOR_GOSSIP_BIND")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = std::env::var("RUNINATOR_GOSSIP_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(5513);
    let ip = bind_address
        .parse::<IpAddr>()
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let socket = bind_discovery_socket(SocketAddr::new(ip, port))
        .map_err(|err| format!("Failed to bind gossip socket: {err}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|err| format!("Failed to configure gossip socket: {err}"))?;

    let mut services = HashMap::<String, WebServiceAnnouncement>::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((len, sender)) => {
                if let Some(service) = parse_announcement(&buffer[..len], sender.ip()) {
                    services.insert(service.service_id.clone(), service);
                    publish_best_service(&app, &state, &services);
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(err) => return Err(format!("Failed to read gossip datagram: {err}")),
        }
    }
}

fn configured_service_url_from_env() -> Result<Option<String>, String> {
    configured_service_url_from_pairs(DIRECT_SERVICE_URL_ENV.iter().filter_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| ((*name).to_string(), value))
    }))
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
    let mut url = Url::parse(value.trim()).map_err(|err| err.to_string())?;
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
            return;
        }
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
