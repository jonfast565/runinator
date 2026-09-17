mod errors;

use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, ToSocketAddrs},
    sync::Arc,
    time::{Duration, Instant},
};

use base64::Engine;
use reqwest::{
    Method, Url,
    blocking::{Client, RequestBuilder},
    header::{HeaderMap, HeaderName, HeaderValue},
    redirect,
};
use runinator_models::{
    errors::SendableError,
    json,
    providers::{
        ActionAuthenticationAlternative, ActionAuthenticationMetadata, ActionMetadata,
        ExecutionProfileSupport, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
    value::Value,
};
use runinator_plugin::{cancel::CancellationToken, provider::Provider};
use serde::Deserialize;

use errors::{
    FORBIDDEN_TARGET, INVALID_PARAMS, INVALID_RESPONSE, REQUEST_FAILED, UNEXPECTED_STATUS,
};

const ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

fn default_body_format() -> String {
    "json".into()
}

fn execute_request(
    request: &ProviderExecutionRequest,
    params: RequestParams,
) -> Result<TaskExecutionResult, SendableError> {
    execute_request_with_allowed_hosts(request, params, configured_allowed_hosts())
}

fn execute_request_with_allowed_hosts(
    request: &ProviderExecutionRequest,
    params: RequestParams,
    allowed_hosts: BTreeSet<String>,
) -> Result<TaskExecutionResult, SendableError> {
    let method = params.method.to_ascii_uppercase();
    if !ALLOWED_METHODS.contains(&method.as_str()) {
        return Err(INVALID_PARAMS.error(format!("unsupported method '{method}'")));
    }
    let method =
        Method::from_bytes(method.as_bytes()).map_err(|error| INVALID_PARAMS.error(error))?;
    let url = Url::parse(&params.url).map_err(|error| INVALID_PARAMS.error(error))?;
    validate_target(&url, &allowed_hosts)?;

    let timeout = Duration::from_secs(
        params
            .timeout_seconds
            .unwrap_or(request.timeout_secs.max(1) as u64)
            .min(request.timeout_secs.max(1) as u64)
            .max(1),
    );
    let redirect_hosts = allowed_hosts.clone();
    let policy = if params.follow_redirects {
        redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("too many redirects");
            }
            match validate_target(attempt.url(), &redirect_hosts) {
                Ok(()) => attempt.follow(),
                Err(_) => attempt.stop(),
            }
        })
    } else {
        redirect::Policy::none()
    };
    let client = Client::builder()
        .timeout(timeout)
        .redirect(policy)
        .user_agent("runinator-http")
        .build()
        .map_err(|error| REQUEST_FAILED.error(error))?;

    let mut builder = client.request(method, url).query(&params.query);
    builder = apply_headers(
        builder,
        &params.headers,
        &request.credential_injections.headers,
    )?;
    builder = apply_body(builder, &params)?;
    let started = Instant::now();
    let response = builder
        .send()
        .map_err(|error| REQUEST_FAILED.error(error))?;
    let status = response.status().as_u16();
    let headers = response_headers(response.headers());
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bytes = response
        .bytes()
        .map_err(|error| INVALID_RESPONSE.error(error))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(INVALID_RESPONSE.error("response exceeded 16 MiB"));
    }
    let body = decode_response_body(&content_type, &bytes)?;
    if !status_expected(status, params.expect_status.as_deref()) {
        return Err(UNEXPECTED_STATUS.error(format!("received status {status}")));
    }

    Ok(TaskExecutionResult {
        message: Some(format!("HTTP request completed with status {status}")),
        output_json: Some(json!({
            "response": {
                "status": status,
                "headers": headers,
                "body": body,
                "duration_ms": started.elapsed().as_millis() as u64,
            }
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn apply_headers(
    mut builder: RequestBuilder,
    authored: &BTreeMap<String, String>,
    injected: &BTreeMap<String, String>,
) -> Result<RequestBuilder, SendableError> {
    let mut headers = HeaderMap::new();
    for (name, value) in authored.iter().chain(injected) {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| INVALID_PARAMS.error(format!("invalid header name: {error}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|error| INVALID_PARAMS.error(format!("invalid header value: {error}")))?;
        headers.insert(name, value);
    }
    builder = builder.headers(headers);
    Ok(builder)
}

fn apply_body(
    builder: RequestBuilder,
    params: &RequestParams,
) -> Result<RequestBuilder, SendableError> {
    let Some(body) = &params.body else {
        return Ok(builder);
    };
    match params.body_format.as_str() {
        "json" => Ok(builder.json(body)),
        "form" => {
            let object = body
                .as_object()
                .ok_or_else(|| INVALID_PARAMS.error("form body must be an object"))?;
            let values = object
                .iter()
                .map(|(key, value)| {
                    let value = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    (key.clone(), value)
                })
                .collect::<BTreeMap<_, _>>();
            Ok(builder.form(&values))
        }
        "text" => body
            .as_str()
            .map(|body| builder.body(body.to_owned()))
            .ok_or_else(|| INVALID_PARAMS.error("text body must be a string")),
        "bytes" => {
            let encoded = body
                .as_str()
                .ok_or_else(|| INVALID_PARAMS.error("bytes body must be a base64 string"))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|error| INVALID_PARAMS.error(format!("invalid base64 body: {error}")))?;
            Ok(builder.body(bytes))
        }
        other => Err(INVALID_PARAMS.error(format!("unsupported body_format '{other}'"))),
    }
}

fn response_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect()
}

fn decode_response_body(content_type: &str, bytes: &[u8]) -> Result<Value, SendableError> {
    if content_type.contains("json") {
        return serde_json::from_slice::<serde_json::Value>(bytes)
            .map(Value::from)
            .map_err(|error| {
                INVALID_RESPONSE.error(format!("response declared JSON but was invalid: {error}"))
            });
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(Value::String(text.to_owned())),
        Err(_) => Ok(json!({
            "base64": base64::engine::general_purpose::STANDARD.encode(bytes),
            "encoding": "base64",
        })),
    }
}

fn status_expected(status: u16, expected: Option<&[u16]>) -> bool {
    match expected {
        None => (200..300).contains(&status),
        Some([]) => true,
        Some(values) => values.contains(&status),
    }
}

fn configured_allowed_hosts() -> BTreeSet<String> {
    std::env::var("RUNINATOR_HTTP_ALLOWED_HOSTS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn validate_target(url: &Url, allowed_hosts: &BTreeSet<String>) -> Result<(), SendableError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FORBIDDEN_TARGET.error("only http and https URLs are permitted"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| FORBIDDEN_TARGET.error("URL must include a host"))?
        .to_ascii_lowercase();
    if allowed_hosts.contains(&host) {
        return Ok(());
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| FORBIDDEN_TARGET.error("URL must include a resolvable port"))?;
    let addresses = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| FORBIDDEN_TARGET.error(format!("could not resolve target: {error}")))?
        .collect::<Vec<_>>();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| private_address(address.ip()))
    {
        return Err(FORBIDDEN_TARGET.error(
            "private, loopback, link-local, and metadata targets require an explicit host allowlist",
        ));
    }
    Ok(())
}

fn private_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || ip.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

mod http_provider;
pub use http_provider::HttpProvider;

mod request_params;
use request_params::RequestParams;
