#[allow(unused_imports)]
use super::*;

/// holds the otel providers so signals keep flowing for the process lifetime and are flushed on
/// shutdown. the bridged tracing layers (returned separately) borrow nothing from this guard, but
/// dropping it shuts the providers down, so keep it alive in `main` until exit.
#[derive(Default)]
pub struct TelemetryGuard {
    pub(super) tracer_provider: Option<SdkTracerProvider>,
    pub(super) meter_provider: Option<SdkMeterProvider>,
    pub(super) logger_provider: Option<SdkLoggerProvider>,
    // Retaining the asynchronous instrument keeps its callback registered for the provider's
    // lifetime. It deliberately lives in the guard beside that provider.
    pub(super) uptime: Option<ObservableGauge<u64>>,
    pub(super) resource_host_cpu: Option<ObservableGauge<f64>>,
    pub(super) resource_host_memory: Option<ObservableGauge<u64>>,
    pub(super) resource_process_cpu: Option<ObservableGauge<f64>>,
    pub(super) resource_process_memory: Option<ObservableGauge<u64>>,
    pub(super) resource_f64_gauges: Vec<ObservableGauge<f64>>,
    pub(super) resource_u64_gauges: Vec<ObservableGauge<u64>>,
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
