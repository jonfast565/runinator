use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use opentelemetry::metrics::ObservableGauge;
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use runinator_models::errors::SendableError;
use runinator_models::telemetry::ResourceTelemetry;

use crate::resource_telemetry::{TelemetryCollector, host_metadata};

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
    // Retaining the asynchronous instrument keeps its callback registered for the provider's
    // lifetime. It deliberately lives in the guard beside that provider.
    uptime: Option<ObservableGauge<u64>>,
    resource_host_cpu: Option<ObservableGauge<f64>>,
    resource_host_memory: Option<ObservableGauge<u64>>,
    resource_process_cpu: Option<ObservableGauge<f64>>,
    resource_process_memory: Option<ObservableGauge<u64>>,
    resource_f64_gauges: Vec<ObservableGauge<f64>>,
    resource_u64_gauges: Vec<ObservableGauge<u64>>,
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
        self.uptime.take();
        self.resource_host_cpu.take();
        self.resource_host_memory.take();
        self.resource_process_cpu.take();
        self.resource_process_memory.take();
        self.resource_f64_gauges.clear();
        self.resource_u64_gauges.clear();
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
    let meter = opentelemetry::global::meter("runinator-process");
    let started = std::time::Instant::now();
    let service = service_name.to_string();
    let uptime = meter
        .u64_observable_gauge("runinator_process_uptime_seconds")
        .with_unit("s")
        .with_callback(move |observer| {
            observer.observe(
                started.elapsed().as_secs(),
                &[opentelemetry::KeyValue::new("service", service.clone())],
            );
        })
        .build();
    // Resource telemetry already powers replica heartbeats. Export its scalar host/process values
    // as gauges as well, so Prometheus-backed Grafana dashboards can show them for every binary.
    // Each instrument retains the collector through its callback for the provider's lifetime.
    let collector = Arc::new(TelemetryCollector::new());
    let resource_host_cpu = resource_host_cpu_gauge(&meter, collector.clone(), service_name);
    let resource_host_memory = resource_host_memory_gauge(&meter, collector.clone(), service_name);
    let resource_process_cpu = resource_process_cpu_gauge(&meter, collector.clone(), service_name);
    let resource_process_memory =
        resource_process_memory_gauge(&meter, collector.clone(), service_name);
    let (resource_f64_gauges, resource_u64_gauges) =
        additional_resource_gauges(&meter, collector, service_name);

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
            uptime: Some(uptime),
            resource_host_cpu: Some(resource_host_cpu),
            resource_host_memory: Some(resource_host_memory),
            resource_process_cpu: Some(resource_process_cpu),
            resource_process_memory: Some(resource_process_memory),
            resource_f64_gauges,
            resource_u64_gauges,
        },
        tracer: Some(tracer),
        logger_provider: Some(logger_provider),
    })
}

fn resource_host_cpu_gauge(
    meter: &opentelemetry::metrics::Meter,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
) -> ObservableGauge<f64> {
    let service = service_name.to_string();
    meter
        .f64_observable_gauge("runinator_resource_host_cpu_percent")
        .with_unit("%")
        .with_callback(move |observer| {
            observer.observe(
                collector.sample().cpu_percent as f64,
                &[opentelemetry::KeyValue::new("service", service.clone())],
            );
        })
        .build()
}

fn resource_host_memory_gauge(
    meter: &opentelemetry::metrics::Meter,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
) -> ObservableGauge<u64> {
    let service = service_name.to_string();
    meter
        .u64_observable_gauge("runinator_resource_host_memory_used_bytes")
        .with_unit("By")
        .with_callback(move |observer| {
            observer.observe(
                collector.sample().mem_used_bytes,
                &[opentelemetry::KeyValue::new("service", service.clone())],
            );
        })
        .build()
}

fn resource_process_cpu_gauge(
    meter: &opentelemetry::metrics::Meter,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
) -> ObservableGauge<f64> {
    let service = service_name.to_string();
    meter
        .f64_observable_gauge("runinator_resource_process_cpu_percent")
        .with_unit("%")
        .with_callback(move |observer| {
            observer.observe(
                collector.sample().process.cpu_percent as f64,
                &[opentelemetry::KeyValue::new("service", service.clone())],
            );
        })
        .build()
}

fn resource_process_memory_gauge(
    meter: &opentelemetry::metrics::Meter,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
) -> ObservableGauge<u64> {
    let service = service_name.to_string();
    meter
        .u64_observable_gauge("runinator_resource_process_memory_used_bytes")
        .with_unit("By")
        .with_callback(move |observer| {
            observer.observe(
                collector.sample().process.mem_used_bytes,
                &[opentelemetry::KeyValue::new("service", service.clone())],
            );
        })
        .build()
}

fn additional_resource_gauges(
    meter: &opentelemetry::metrics::Meter,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
) -> (Vec<ObservableGauge<f64>>, Vec<ObservableGauge<u64>>) {
    let f64_gauges = vec![
        resource_f64_gauge(
            meter,
            "runinator_resource_host_memory_percent",
            "%",
            collector.clone(),
            service_name,
            |s| s.mem_percent as f64,
        ),
        resource_optional_f64_gauge(
            meter,
            "runinator_resource_load_1",
            collector.clone(),
            service_name,
            |s| s.load_average.as_ref().map(|v| v.one),
        ),
        resource_optional_f64_gauge(
            meter,
            "runinator_resource_load_5",
            collector.clone(),
            service_name,
            |s| s.load_average.as_ref().map(|v| v.five),
        ),
        resource_optional_f64_gauge(
            meter,
            "runinator_resource_load_15",
            collector.clone(),
            service_name,
            |s| s.load_average.as_ref().map(|v| v.fifteen),
        ),
        resource_f64_gauge(
            meter,
            "runinator_resource_network_receive_bytes_per_second",
            "By/s",
            collector.clone(),
            service_name,
            |s| s.network.rx_bytes_per_sec,
        ),
        resource_f64_gauge(
            meter,
            "runinator_resource_network_transmit_bytes_per_second",
            "By/s",
            collector.clone(),
            service_name,
            |s| s.network.tx_bytes_per_sec,
        ),
        resource_disk_f64_gauge(
            meter,
            "runinator_resource_disk_read_bytes_per_second",
            collector.clone(),
            service_name,
            |d| d.read_bytes_per_sec,
        ),
        resource_disk_f64_gauge(
            meter,
            "runinator_resource_disk_written_bytes_per_second",
            collector.clone(),
            service_name,
            |d| d.written_bytes_per_sec,
        ),
        resource_gpu_f64_gauge(
            meter,
            "runinator_resource_gpu_utilization_percent",
            collector.clone(),
            service_name,
            |g| g.utilization_percent.map(f64::from),
        ),
    ];
    let u64_gauges = vec![
        resource_u64_gauge(
            meter,
            "runinator_resource_host_memory_total_bytes",
            collector.clone(),
            service_name,
            |s| s.mem_total_bytes,
        ),
        resource_u64_gauge(
            meter,
            "runinator_resource_swap_used_bytes",
            collector.clone(),
            service_name,
            |s| s.swap_used_bytes,
        ),
        resource_u64_gauge(
            meter,
            "runinator_resource_swap_total_bytes",
            collector.clone(),
            service_name,
            |s| s.swap_total_bytes,
        ),
        resource_u64_gauge(
            meter,
            "runinator_resource_network_receive_total_bytes",
            collector.clone(),
            service_name,
            |s| s.network.rx_total_bytes,
        ),
        resource_u64_gauge(
            meter,
            "runinator_resource_network_transmit_total_bytes",
            collector.clone(),
            service_name,
            |s| s.network.tx_total_bytes,
        ),
        resource_disk_u64_gauge(
            meter,
            "runinator_resource_disk_total_bytes",
            collector.clone(),
            service_name,
            |d| d.total_bytes,
        ),
        resource_disk_u64_gauge(
            meter,
            "runinator_resource_disk_available_bytes",
            collector.clone(),
            service_name,
            |d| d.available_bytes,
        ),
        resource_gpu_u64_gauge(
            meter,
            "runinator_resource_gpu_memory_used_bytes",
            collector.clone(),
            service_name,
            |g| g.mem_used_bytes,
        ),
        resource_gpu_u64_gauge(
            meter,
            "runinator_resource_gpu_memory_total_bytes",
            collector,
            service_name,
            |g| g.mem_total_bytes,
        ),
        static_u64_gauge(
            meter,
            "runinator_resource_host_logical_cores",
            "1",
            host_metadata().logical_cores as u64,
            service_name,
        ),
        static_u64_gauge(
            meter,
            "runinator_resource_host_boot_time_seconds",
            "s",
            host_metadata().boot_time_unix,
            service_name,
        ),
        host_info_gauge(meter, service_name),
    ];
    (f64_gauges, u64_gauges)
}

fn static_u64_gauge(
    meter: &opentelemetry::metrics::Meter,
    name: &'static str,
    unit: &'static str,
    value: u64,
    service_name: &str,
) -> ObservableGauge<u64> {
    let service = service_name.to_string();
    meter
        .u64_observable_gauge(name)
        .with_unit(unit)
        .with_callback(move |o| {
            o.observe(
                value,
                &[opentelemetry::KeyValue::new("service", service.clone())],
            )
        })
        .build()
}

fn host_info_gauge(
    meter: &opentelemetry::metrics::Meter,
    service_name: &str,
) -> ObservableGauge<u64> {
    let host = host_metadata();
    let attributes = vec![
        opentelemetry::KeyValue::new("service", service_name.to_string()),
        opentelemetry::KeyValue::new(
            "host_name",
            host.host_name.unwrap_or_else(|| "unknown".to_string()),
        ),
        opentelemetry::KeyValue::new("os", host.os.unwrap_or_else(|| "unknown".to_string())),
        opentelemetry::KeyValue::new(
            "os_version",
            host.os_version.unwrap_or_else(|| "unknown".to_string()),
        ),
        opentelemetry::KeyValue::new(
            "kernel_version",
            host.kernel_version.unwrap_or_else(|| "unknown".to_string()),
        ),
        opentelemetry::KeyValue::new("cpu_arch", host.cpu_arch),
        opentelemetry::KeyValue::new(
            "cpu_brand",
            host.cpu_brand.unwrap_or_else(|| "unknown".to_string()),
        ),
        opentelemetry::KeyValue::new(
            "physical_cores",
            host.physical_cores.unwrap_or_default() as i64,
        ),
    ];
    meter
        .u64_observable_gauge("runinator_resource_host_info")
        .with_callback(move |o| o.observe(1, &attributes))
        .build()
}

fn resource_f64_gauge<F>(
    meter: &opentelemetry::metrics::Meter,
    name: &'static str,
    unit: &'static str,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
    value: F,
) -> ObservableGauge<f64>
where
    F: Fn(&ResourceTelemetry) -> f64 + Send + Sync + 'static,
{
    let service = service_name.to_string();
    meter
        .f64_observable_gauge(name)
        .with_unit(unit)
        .with_callback(move |o| {
            let sample = collector.sample();
            o.observe(
                value(&sample),
                &[opentelemetry::KeyValue::new("service", service.clone())],
            );
        })
        .build()
}

fn resource_optional_f64_gauge<F>(
    meter: &opentelemetry::metrics::Meter,
    name: &'static str,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
    value: F,
) -> ObservableGauge<f64>
where
    F: Fn(&ResourceTelemetry) -> Option<f64> + Send + Sync + 'static,
{
    let service = service_name.to_string();
    meter
        .f64_observable_gauge(name)
        .with_callback(move |o| {
            let sample = collector.sample();
            if let Some(value) = value(&sample) {
                o.observe(
                    value,
                    &[opentelemetry::KeyValue::new("service", service.clone())],
                );
            }
        })
        .build()
}

fn resource_u64_gauge<F>(
    meter: &opentelemetry::metrics::Meter,
    name: &'static str,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
    value: F,
) -> ObservableGauge<u64>
where
    F: Fn(&ResourceTelemetry) -> u64 + Send + Sync + 'static,
{
    let service = service_name.to_string();
    meter
        .u64_observable_gauge(name)
        .with_unit("By")
        .with_callback(move |o| {
            let sample = collector.sample();
            o.observe(
                value(&sample),
                &[opentelemetry::KeyValue::new("service", service.clone())],
            );
        })
        .build()
}

fn resource_disk_f64_gauge<F>(
    meter: &opentelemetry::metrics::Meter,
    name: &'static str,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
    value: F,
) -> ObservableGauge<f64>
where
    F: Fn(&runinator_models::telemetry::DiskTelemetry) -> f64 + Send + Sync + 'static,
{
    let service = service_name.to_string();
    meter
        .f64_observable_gauge(name)
        .with_unit("By/s")
        .with_callback(move |o| {
            for disk in collector.sample().disks {
                o.observe(
                    value(&disk),
                    &[
                        opentelemetry::KeyValue::new("service", service.clone()),
                        opentelemetry::KeyValue::new("mount_point", disk.mount_point),
                    ],
                );
            }
        })
        .build()
}

fn resource_disk_u64_gauge<F>(
    meter: &opentelemetry::metrics::Meter,
    name: &'static str,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
    value: F,
) -> ObservableGauge<u64>
where
    F: Fn(&runinator_models::telemetry::DiskTelemetry) -> u64 + Send + Sync + 'static,
{
    let service = service_name.to_string();
    meter
        .u64_observable_gauge(name)
        .with_unit("By")
        .with_callback(move |o| {
            for disk in collector.sample().disks {
                o.observe(
                    value(&disk),
                    &[
                        opentelemetry::KeyValue::new("service", service.clone()),
                        opentelemetry::KeyValue::new("mount_point", disk.mount_point),
                    ],
                );
            }
        })
        .build()
}

fn resource_gpu_f64_gauge<F>(
    meter: &opentelemetry::metrics::Meter,
    name: &'static str,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
    value: F,
) -> ObservableGauge<f64>
where
    F: Fn(&runinator_models::telemetry::GpuTelemetry) -> Option<f64> + Send + Sync + 'static,
{
    let service = service_name.to_string();
    meter
        .f64_observable_gauge(name)
        .with_unit("%")
        .with_callback(move |o| {
            for gpu in collector.sample().gpus {
                if let Some(value) = value(&gpu) {
                    o.observe(
                        value,
                        &[
                            opentelemetry::KeyValue::new("service", service.clone()),
                            opentelemetry::KeyValue::new("gpu", gpu.name),
                        ],
                    );
                }
            }
        })
        .build()
}

fn resource_gpu_u64_gauge<F>(
    meter: &opentelemetry::metrics::Meter,
    name: &'static str,
    collector: Arc<TelemetryCollector>,
    service_name: &str,
    value: F,
) -> ObservableGauge<u64>
where
    F: Fn(&runinator_models::telemetry::GpuTelemetry) -> Option<u64> + Send + Sync + 'static,
{
    let service = service_name.to_string();
    meter
        .u64_observable_gauge(name)
        .with_unit("By")
        .with_callback(move |o| {
            for gpu in collector.sample().gpus {
                if let Some(value) = value(&gpu) {
                    o.observe(
                        value,
                        &[
                            opentelemetry::KeyValue::new("service", service.clone()),
                            opentelemetry::KeyValue::new("gpu", gpu.name),
                        ],
                    );
                }
            }
        })
        .build()
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
    // The Prometheus exporter uses resource attributes as series labels. Include a stable
    // per-process identity so replicas of the same service do not produce duplicate samples
    // with the same label set when they export the same instrument to one collector.
    let instance_id = std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|hostname| format!("{hostname}-{}", std::process::id()))
        .unwrap_or_else(|| format!("pid-{}", std::process::id()));

    Resource::builder()
        .with_service_name(service_name.to_string())
        .with_attribute(opentelemetry::KeyValue::new(
            opentelemetry_semantic_conventions::resource::SERVICE_INSTANCE_ID,
            instance_id,
        ))
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
