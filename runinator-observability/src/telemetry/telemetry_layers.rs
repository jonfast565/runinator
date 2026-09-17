#[allow(unused_imports)]
use super::*;

pub struct TelemetryLayers {
    pub guard: TelemetryGuard,
    pub tracer: Option<SdkTracer>,
    pub logger_provider: Option<SdkLoggerProvider>,
}
