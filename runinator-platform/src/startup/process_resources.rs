#[allow(unused_imports)]
use super::*;

pub struct ProcessResources {
    pub(super) _telemetry: TelemetryGuard,
    pub(super) shutdown: Shutdown,
}

impl ProcessResources {
    pub fn start(name: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(Self {
            _telemetry: startup(name)?,
            shutdown: Shutdown::install(),
        })
    }

    pub fn shutdown(&self) -> &Shutdown {
        &self.shutdown
    }
}
