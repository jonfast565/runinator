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
        physical_cores: system.physical_core_count(),
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
pub struct TelemetryCollector {
    system: Mutex<System>,
    // network/disk counters plus the instant of the previous sample, kept together so the rate math
    // shares one elapsed window and stays consistent across concurrent callers.
    io: Mutex<IoState>,
    // this process's pid, used to attribute per-process cpu/memory; absent if it can't be resolved.
    pid: Option<Pid>,
    // present only when libnvidia-ml loaded and initialized; absent on non-nvidia hosts.
    nvml: Option<Nvml>,
}

struct IoState {
    networks: Networks,
    disks: Disks,
    last_sample: Option<Instant>,
}

impl TelemetryCollector {
    /// build a collector, attempting nvml init once. gpu telemetry is silently disabled when no
    /// nvidia management library or device is available.
    pub fn new() -> Self {
        let nvml = match Nvml::init() {
            Ok(nvml) => Some(nvml),
            Err(err) => {
                debug!("gpu telemetry disabled (nvml init failed): {err}");
                None
            }
        };
        Self {
            system: Mutex::new(System::new()),
            io: Mutex::new(IoState {
                networks: Networks::new_with_refreshed_list(),
                disks: Disks::new_with_refreshed_list(),
                last_sample: None,
            }),
            pid: sysinfo::get_current_pid().ok(),
            nvml,
        }
    }

    /// take a fresh snapshot. cpu and per-process usage reflect activity since the previous sample on
    /// this collector, so the first sample after construction may read low.
    pub fn sample(&self) -> ResourceTelemetry {
        let (
            cpu_percent,
            mem_used_bytes,
            mem_total_bytes,
            swap_used_bytes,
            swap_total_bytes,
            process,
        ) = {
            let mut system = match self.system.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            system.refresh_cpu_usage();
            system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram().with_swap());
            let process = self.sample_process(&mut system);
            (
                system.global_cpu_usage(),
                system.used_memory(),
                system.total_memory(),
                system.used_swap(),
                system.total_swap(),
                process,
            )
        };
        let mem_percent = if mem_total_bytes > 0 {
            (mem_used_bytes as f64 / mem_total_bytes as f64 * 100.0) as f32
        } else {
            0.0
        };
        let (network, disks) = self.sample_io();
        ResourceTelemetry {
            cpu_percent,
            mem_used_bytes,
            mem_total_bytes,
            mem_percent,
            swap_used_bytes,
            swap_total_bytes,
            load_average: load_average(),
            process,
            network,
            disks,
            gpus: self.sample_gpus(),
            sampled_at: Utc::now(),
        }
    }

    // refresh and read this process's own cpu/memory. requires the caller to hold the system lock.
    fn sample_process(&self, system: &mut System) -> ProcessTelemetry {
        let Some(pid) = self.pid else {
            return ProcessTelemetry::default();
        };
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        match system.process(pid) {
            Some(process) => ProcessTelemetry {
                cpu_percent: process.cpu_usage(),
                mem_used_bytes: process.memory(),
            },
            None => ProcessTelemetry::default(),
        }
    }

    fn sample_io(&self) -> (NetworkTelemetry, Vec<DiskTelemetry>) {
        let mut io = match self.io.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        // refresh without dropping entries that briefly disappear, so counters stay monotonic.
        io.networks.refresh(false);
        io.disks.refresh(false);
        let now = Instant::now();
        let secs = io
            .last_sample
            .map(|prev| now.duration_since(prev).as_secs_f64())
            .filter(|secs| *secs > 0.0);
        io.last_sample = Some(now);
        let rate = |delta: u64| secs.map(|secs| delta as f64 / secs).unwrap_or(0.0);

        let mut rx_delta = 0u64;
        let mut tx_delta = 0u64;
        let mut rx_total_bytes = 0u64;
        let mut tx_total_bytes = 0u64;
        for (_, data) in io.networks.iter() {
            rx_delta = rx_delta.saturating_add(data.received());
            tx_delta = tx_delta.saturating_add(data.transmitted());
            rx_total_bytes = rx_total_bytes.saturating_add(data.total_received());
            tx_total_bytes = tx_total_bytes.saturating_add(data.total_transmitted());
        }
        let network = NetworkTelemetry {
            rx_bytes_per_sec: rate(rx_delta),
            tx_bytes_per_sec: rate(tx_delta),
            rx_total_bytes,
            tx_total_bytes,
        };

        let disks = io
            .disks
            .iter()
            .map(|disk| {
                let usage = disk.usage();
                DiskTelemetry {
                    mount_point: disk.mount_point().to_string_lossy().into_owned(),
                    total_bytes: disk.total_space(),
                    available_bytes: disk.available_space(),
                    read_bytes_per_sec: rate(usage.read_bytes),
                    written_bytes_per_sec: rate(usage.written_bytes),
                }
            })
            .collect();

        (network, disks)
    }

    fn sample_gpus(&self) -> Vec<GpuTelemetry> {
        let Some(nvml) = self.nvml.as_ref() else {
            return Vec::new();
        };
        let count = match nvml.device_count() {
            Ok(count) => count,
            Err(err) => {
                debug!("gpu telemetry: device_count failed: {err}");
                return Vec::new();
            }
        };
        let mut gpus = Vec::with_capacity(count as usize);
        for index in 0..count {
            let Ok(device) = nvml.device_by_index(index) else {
                continue;
            };
            let name = device.name().unwrap_or_else(|_| format!("gpu-{index}"));
            let utilization_percent = device
                .utilization_rates()
                .ok()
                .map(|rates| rates.gpu as f32);
            let (mem_used_bytes, mem_total_bytes) = match device.memory_info() {
                Ok(info) => (Some(info.used), Some(info.total)),
                Err(_) => (None, None),
            };
            gpus.push(GpuTelemetry {
                name,
                utilization_percent,
                mem_used_bytes,
                mem_total_bytes,
            });
        }
        gpus
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "resource_telemetry_tests.rs"]
mod tests;
