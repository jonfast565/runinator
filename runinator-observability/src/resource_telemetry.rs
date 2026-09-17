// samples host cpu/memory (via sysinfo) and gpu (best-effort via nvml) into the shared
// `ResourceTelemetry` wire struct. one collector is built per service process and reused across
// heartbeats so cpu deltas accumulate between samples.

use std::sync::Mutex;
use std::time::Instant;

use chrono::Utc;
use log::debug;
use nvml_wrapper::Nvml;
use runinator_models::telemetry::{
    DiskTelemetry, GpuTelemetry, HostMetadata, LoadAverage, NetworkTelemetry, ProcessTelemetry,
    ResourceTelemetry,
};
use runinator_models::value::Value;
use sysinfo::{
    Disks, MemoryRefreshKind, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System,
};

/// merge a fresh resource-telemetry sample into a copy of `base` under the `telemetry` key. used so
/// every heartbeat carries live cpu/ram/gpu numbers alongside the replica's static attributes.
pub fn attributes_with_telemetry(base: &Value, collector: &TelemetryCollector) -> Value {
    let mut attributes = match base {
        Value::Object(_) => base.clone(),
        _ => Value::Object(Default::default()),
    };
    let snapshot = serde_json::to_value(collector.sample())
        .map(Value::from)
        .unwrap_or(Value::Null);
    if let Some(object) = attributes.as_object_mut() {
        object.insert("telemetry".to_string(), snapshot);
    }
    attributes
}

/// merge static host facts into a copy of `base` under the `host` key. meant for the one-time
/// replica registration attributes, since these values do not change over a process lifetime.
pub fn attributes_with_host_metadata(base: &Value) -> Value {
    let mut attributes = match base {
        Value::Object(_) => base.clone(),
        _ => Value::Object(Default::default()),
    };
    let host = serde_json::to_value(host_metadata())
        .map(Value::from)
        .unwrap_or(Value::Null);
    if let Some(object) = attributes.as_object_mut() {
        object.insert("host".to_string(), host);
    }
    attributes
}

/// collect static host facts (os, cpu, memory size, boot time) for replica registration.
pub fn host_metadata() -> HostMetadata {
    let mut system = System::new();
    system.refresh_cpu_all();
    system.refresh_memory();
    let cpu_brand = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|brand| !brand.is_empty());
    HostMetadata {
        host_name: System::host_name(),
        os: System::name(),
        os_version: System::long_os_version(),
        kernel_version: System::kernel_version(),
        cpu_arch: System::cpu_arch(),
        cpu_brand,
        physical_cores: System::physical_core_count(),
        logical_cores: system.cpus().len(),
        mem_total_bytes: system.total_memory(),
        boot_time_unix: System::boot_time(),
    }
}

// 1/5/15-minute load average, reported only where the platform supports it.
fn load_average() -> Option<LoadAverage> {
    #[cfg(unix)]
    {
        let load = System::load_average();
        Some(LoadAverage {
            one: load.one,
            five: load.five,
            fifteen: load.fifteen,
        })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// process-lived sampler of host resource usage. cheap to clone-share behind an `Arc`.

#[cfg(test)]
#[path = "resource_telemetry_tests.rs"]
mod tests;

mod telemetry_collector;
pub use telemetry_collector::TelemetryCollector;

mod io_state;
use io_state::IoState;
