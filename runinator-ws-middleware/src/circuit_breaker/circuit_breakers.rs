#[allow(unused_imports)]
use super::*;

/// Stateful selector containing one library-owned breaker per request family.
#[derive(Clone)]
pub struct CircuitBreakers {
    pub(super) enabled: bool,
    pub(super) cooldown: Duration,
    pub(super) read_query: HttpCircuitLayer,
    pub(super) write_control: HttpCircuitLayer,
    pub(super) external_ingress: HttpCircuitLayer,
}

impl CircuitBreakers {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        config
            .validate()
            .expect("inbound circuit breaker configuration was validated at startup");
        Self {
            enabled: config.enabled,
            cooldown: config.cooldown,
            read_query: make_breaker(config, CircuitFamily::ReadQuery),
            write_control: make_breaker(config, CircuitFamily::WriteControl),
            external_ingress: make_breaker(config, CircuitFamily::ExternalIngress),
        }
    }

    pub(super) fn select(
        &self,
        request: &Request<Body>,
    ) -> Option<(CircuitFamily, HttpCircuitLayer)> {
        if !self.enabled || is_bypassed(request) {
            return None;
        }
        let family = if is_external_ingress(request) {
            CircuitFamily::ExternalIngress
        } else if matches!(*request.method(), Method::GET | Method::HEAD) {
            CircuitFamily::ReadQuery
        } else {
            CircuitFamily::WriteControl
        };
        let layer = match family {
            CircuitFamily::ReadQuery => self.read_query.clone(),
            CircuitFamily::WriteControl => self.write_control.clone(),
            CircuitFamily::ExternalIngress => self.external_ingress.clone(),
        };
        Some((family, layer))
    }
}
