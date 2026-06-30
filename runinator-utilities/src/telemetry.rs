use std::collections::HashMap;
use std::env;

use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use runinator_models::errors::SendableError;

// the tracer name used for the per-binary tracing-opentelemetry bridge.
const TRACER_NAME: &str = "runinator";

/// holds the otel providers so signals keep flowing for the process lifetime and are flushed on
/// shutdown. the bridged tracing layers (returned separately) borrow nothing from this guard, but
/// dropping it shuts the providers down, so keep it alive in `main` until exit.
#[derive(Default)]
pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl TelemetryGuard {
    /// a guard owning no providers; returned when otel is disabled or already initialized.
    pub fn disabled() -> Self {
        Self::default()
    }

    /// true when at least one signal provider was installed.
    pub fn is_enabled(&self) -> bool {
        self.tracer_provider.is_some()
            || self.meter_provider.is_some()
            || self.logger_provider.is_some()
    }

    /// flush and shut the providers down. idempotent; called automatically on drop.
    pub fn shutdown(&mut self) {
        if let Some(provider) = self.tracer_provider.take() {
            let _ = provider.shutdown();
        }
        if let Some(provider) = self.meter_provider.take() {
            let _ = provider.shutdown();
        }
        if let Some(provider) = self.logger_provider.take() {
            let _ = provider.shutdown();
        }
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// the tracing layers bridged to otel, paired with the guard that keeps the providers alive. the
/// caller composes the layers into the global subscriber and retains the guard.
pub struct TelemetryLayers {
    pub guard: TelemetryGuard,
    pub tracer: Option<SdkTracer>,
    pub logger_provider: Option<SdkLoggerProvider>,
}

/// install the global w3c trace-context propagator and, when otel is configured, build the otlp
/// trace/metric/log providers for `service_name`. returns the bridged trace/log layers for the
/// subscriber plus a guard that flushes on drop. a no-op (disabled) result is returned when otel is
/// turned off, so the existing stdout/file logging path is unchanged.
pub fn init(service_name: &str) -> Result<TelemetryLayers, SendableError> {
    // w3c propagation is cheap and harmless when disabled, so always install it. this lets the http
    // and broker paths inject/extract `traceparent` uniformly regardless of exporter state.
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    if !is_enabled() {
        return Ok(TelemetryLayers {
            guard: TelemetryGuard::disabled(),
            tracer: None,
            logger_provider: None,
        });
    }

    let resource = build_resource(service_name);

    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .map_err(to_sendable)?;
    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource.clone())
        .build();
    let tracer = tracer_provider.tracer(TRACER_NAME);
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .build()
        .map_err(to_sendable)?;
    let meter_provider = SdkMeterProvider::builder()
        .with_periodic_exporter(metric_exporter)
        .with_resource(resource.clone())
        .build();
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    let log_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_http()
        .build()
        .map_err(to_sendable)?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_batch_exporter(log_exporter)
        .with_resource(resource)
        .build();

    Ok(TelemetryLayers {
        guard: TelemetryGuard {
            tracer_provider: Some(tracer_provider),
            meter_provider: Some(meter_provider),
            logger_provider: Some(logger_provider.clone()),
        },
        tracer: Some(tracer),
        logger_provider: Some(logger_provider),
    })
}

/// otel is on when an otlp endpoint is configured and the sdk is not explicitly disabled. this
/// mirrors the standard otel sdk environment contract.
fn is_enabled() -> bool {
    if env_flag_true("OTEL_SDK_DISABLED") {
        return false;
    }
    has_value("OTEL_EXPORTER_OTLP_ENDPOINT")
        || has_value("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT")
        || has_value("OTEL_EXPORTER_OTLP_METRICS_ENDPOINT")
        || has_value("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT")
}

// build the resource describing this binary. `OTEL_SERVICE_NAME`/`OTEL_RESOURCE_ATTRIBUTES` in the
// environment still win via the sdk's env detector; the passed name is the default service.name.
fn build_resource(service_name: &str) -> Resource {
    Resource::builder()
        .with_service_name(service_name.to_string())
        .with_attribute(opentelemetry::KeyValue::new(
            opentelemetry_semantic_conventions::resource::SERVICE_VERSION,
            env!("CARGO_PKG_VERSION"),
        ))
        .build()
}

fn has_value(key: &str) -> bool {
    env::var(key).is_ok_and(|value| !value.trim().is_empty())
}

fn env_flag_true(key: &str) -> bool {
    env::var(key)
        .is_ok_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

fn to_sendable<E: std::error::Error + Send + Sync + 'static>(err: E) -> SendableError {
    Box::new(err)
}

/// w3c trace-context carried as a serde-friendly string map across broker messages. backend-neutral
/// so any broker backend serializes it without special handling; empty when otel is off.
pub type TraceContext = HashMap<String, String>;

struct MapInjector<'a>(&'a mut TraceContext);

impl Injector for MapInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
}

struct MapExtractor<'a>(&'a TraceContext);

impl Extractor for MapExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }
}

/// capture the active span's trace context into a carrier for embedding in a broker message. empty
/// when otel is disabled or no span is active, so producers can always call it unconditionally.
pub fn current_trace_context() -> TraceContext {
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let context = tracing::Span::current().context();
    let mut carrier = TraceContext::new();
    opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut MapInjector(&mut carrier));
    });
    carrier
}

/// re-parent `span` onto the trace context carried in a broker message, linking the consumer's work
/// to the producer's trace. a no-op when the carrier is empty (sender had otel off).
pub fn apply_trace_context(span: &tracing::Span, carrier: &TraceContext) {
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    if carrier.is_empty() {
        return;
    }
    let parent = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&MapExtractor(carrier))
    });
    // errors only when no otel layer is installed (otel off); the local span is still valid.
    let _ = span.set_parent(parent);
}

/// re-parent `span` onto the w3c context carried in inbound http headers (e.g. `traceparent`), so a
/// server-side request span continues a caller's distributed trace. a no-op when otel is off.
pub fn apply_http_context(span: &tracing::Span, headers: &http::HeaderMap) {
    use opentelemetry_http::HeaderExtractor;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let parent = opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(headers))
    });
    // errors only when no otel layer is installed (otel off); the local span is still valid.
    let _ = span.set_parent(parent);
}

/// inject the active span's w3c trace context (e.g. `traceparent`) into outbound http headers so the
/// receiving service can continue this trace. a no-op when otel is off (no headers added).
pub fn inject_into_headers(headers: &mut http::HeaderMap) {
    use opentelemetry_http::HeaderInjector;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let context = tracing::Span::current().context();
    opentelemetry::global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut HeaderInjector(headers));
    });
}
