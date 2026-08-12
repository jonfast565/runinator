//! first-start enrollment and issued-credential persistence.

use std::{collections::BTreeMap, fmt, sync::Arc, time::Duration};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_auth::enroll::EnrollToken;
use runinator_models::auth::{AgentEnrollmentRequestBody, EnrollAgentRequest};
use runinator_models::errors::SendableError;
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::WebPkiSupportedAlgorithms,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x509_parser::parse_x509_certificate;

use crate::agent::config::{AgentRuntimeConfig, LocatorMode};
use crate::agent::relay::{derive_relay_url, derive_relay_url_with_path};

#[derive(Debug, Serialize, Deserialize)]
struct StoredAgentCredential {
    service_url: String,
    api_key: String,
    instance_id: String,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    cluster_id: Option<uuid::Uuid>,
}

/// load a previously issued credential, or redeem the one-time token when this is the first start.
/// the token is held only in memory; the persisted file contains the issued API key and identity.
pub async fn prepare_agent_credentials(
    config: &mut AgentRuntimeConfig,
) -> Result<(), SendableError> {
    if config
        .api_key
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        return Ok(());
    }
    if let Some(mut stored) = read_stored(config)? {
        let relay_path = if config.locator_mode == LocatorMode::Discover {
            let cluster_id = stored.cluster_id.ok_or_else(|| {
                crate::errors::API_CLIENT.error(
                    "the stored legacy credential is not bound to a cluster; configure --service-url once before enabling discovery",
                )
            })?;
            let service = discover_service(config, cluster_id).await?;
            stored.service_url = runinator_comm::discovery::web::web_service_base_url(&service);
            Some(service.relay_path)
        } else {
            None
        };
        apply(config, stored);
        if let Some(relay_path) = relay_path
            && config.broker.broker_backend == "ws"
        {
            config.broker.broker_endpoint =
                derive_relay_url_with_path(&config.service_url, &relay_path)?;
            config.broker_description = format!("relay via {}", config.broker.broker_endpoint);
        }
        return Ok(());
    }
    let Some(raw_token) = config.enrollment_token.take() else {
        if config.locator_mode == LocatorMode::Discover {
            return Err(crate::errors::API_CLIENT.error(
                "automatic discovery requires an enrollment token bound to a cluster; choose a candidate explicitly when enrolling without a token",
            ));
        }
        return Ok(());
    };
    let mut token =
        EnrollToken::decode(&raw_token).map_err(|err| crate::errors::API_CLIENT.error(err))?;
    if config.locator_mode == LocatorMode::Discover {
        let cluster_id = token.cluster_id.ok_or_else(|| {
            crate::errors::API_CLIENT.error(
                "this legacy enrollment token is not bound to a cluster and cannot use automatic discovery",
            )
        })?;
        let service = discover_service(config, cluster_id).await?;
        config.service_url = runinator_comm::discovery::web::web_service_base_url(&service);
        token.service_url = config.service_url.clone();
        if token.spki_pin.is_none() {
            token.spki_pin = service.spki_pin.clone();
        }
        if config.broker.broker_backend == "ws" {
            config.broker.broker_endpoint =
                derive_relay_url_with_path(&config.service_url, &service.relay_path)?;
            config.broker_description = format!("relay via {}", config.broker.broker_endpoint);
        }
    }
    let body = AgentEnrollmentRequestBody {
        instance_id: config.instance_id.clone(),
        display_name: config.display_name.clone(),
        labels: config.labels.clone(),
    };
    let canonical = serde_json::to_vec(&body)?;
    let request = EnrollAgentRequest {
        token_id: token.token_id.clone(),
        request_body: body,
        proof: URL_SAFE_NO_PAD.encode(token.proof(&canonical)),
    };
    let client = enrollment_client(&token)?;
    let response = client
        .enroll_agent(&request)
        .await
        .map_err(|err| crate::errors::API_CLIENT.error(err))?;
    let stored = StoredAgentCredential {
        service_url: if config.locator_mode == LocatorMode::Discover {
            config.service_url.clone()
        } else {
            response.service_url
        },
        api_key: response.api_key,
        instance_id: config.instance_id.clone(),
        labels: response.labels,
        cluster_id: token.cluster_id,
    };
    let bytes = serde_json::to_vec(&stored)?;
    runinator_utilities::secret_file::write_secret_file_atomic(&config.credential_file, &bytes)
        .map_err(|err| crate::errors::API_CLIENT.error(err))?;
    apply(config, stored);
    Ok(())
}

async fn discover_service(
    config: &AgentRuntimeConfig,
    cluster_id: uuid::Uuid,
) -> Result<runinator_comm::WebServiceAnnouncement, SendableError> {
    let discovery = runinator_comm::discovery::web::start_web_service_listener(
        &config.gossip_bind,
        config.gossip_port,
    )
    .await
    .map_err(|err| crate::errors::API_CLIENT.error(err))?;
    discovery.wait_for_cluster_url(cluster_id).await;
    discovery
        .current_service_for_cluster(cluster_id)
        .await
        .ok_or_else(|| crate::errors::API_CLIENT.error("matching discovery candidate disappeared"))
}

fn enrollment_client(token: &EnrollToken) -> Result<AsyncApiClient<StaticLocator>, SendableError> {
    let locator = StaticLocator::new(token.service_url.clone());
    let Some(raw_pin) = token.spki_pin.as_deref() else {
        return AsyncApiClient::new(locator).map_err(|err| crate::errors::API_CLIENT.error(err));
    };
    if !token.service_url.starts_with("https://") {
        return Err(crate::errors::API_CLIENT.error("an SPKI pin requires an https service URL"));
    }
    let pin = decode_spki_pin(raw_pin)?;
    let provider = rustls::crypto::ring::default_provider();
    let verifier = Arc::new(PinnedServerVerifier {
        pin,
        algorithms: provider.signature_verification_algorithms,
    });
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|err| crate::errors::API_CLIENT.error(err))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .use_preconfigured_tls(tls)
        .build()
        .map_err(|err| crate::errors::API_CLIENT.error(err))?;
    Ok(AsyncApiClient::with_client(locator, client))
}

fn decode_spki_pin(raw: &str) -> Result<[u8; 32], SendableError> {
    let encoded = raw.strip_prefix("sha256/").unwrap_or(raw);
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .or_else(|_| URL_SAFE_NO_PAD.decode(encoded))
        .map_err(|_| crate::errors::API_CLIENT.error("invalid SPKI pin"))?;
    decoded
        .try_into()
        .map_err(|_| crate::errors::API_CLIENT.error("invalid SPKI pin"))
}

struct PinnedServerVerifier {
    pin: [u8; 32],
    algorithms: WebPkiSupportedAlgorithms,
}

impl fmt::Debug for PinnedServerVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("PinnedServerVerifier").finish()
    }
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let (_, certificate) = parse_x509_certificate(end_entity.as_ref()).map_err(|_| {
            rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding)
        })?;
        let actual: [u8; 32] = Sha256::digest(certificate.public_key().raw).into();
        if actual != self.pin {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, signature, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, signature, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

fn read_stored(
    config: &AgentRuntimeConfig,
) -> Result<Option<StoredAgentCredential>, SendableError> {
    let raw = match std::fs::read(&config.credential_file) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(crate::errors::API_CLIENT.error(err)),
    };
    let stored = serde_json::from_slice(&raw).map_err(|err| {
        crate::errors::API_CLIENT.error(format!("stored agent credential is invalid: {err}"))
    })?;
    Ok(Some(stored))
}

fn apply(config: &mut AgentRuntimeConfig, stored: StoredAgentCredential) {
    config.service_url = stored.service_url;
    config.api_key = Some(stored.api_key.clone());
    config.instance_id = stored.instance_id;
    config.labels = stored.labels;
    if config.broker.broker_backend == "ws" {
        if let Ok(endpoint) = derive_relay_url(&config.service_url) {
            config.broker.broker_endpoint = endpoint;
        }
        config.broker.api_key = Some(stored.api_key);
        config.broker_description = format!("relay via {}", config.broker.broker_endpoint);
    }
}

#[cfg(test)]
#[path = "enroll_tests.rs"]
mod tests;
