#[allow(unused_imports)]
use super::*;

/// A compact, GUI-friendly projection of one resource sample. Keeping only rendered values avoids
/// coupling desktop UI state to the wire model while retaining every graph from `--tui`.
#[derive(Debug, Clone, Default)]
pub struct ResourceSample {
    pub host_cpu_percent: f32,
    pub host_mem_percent: f32,
    pub host_mem_used_bytes: u64,
    pub host_mem_total_bytes: u64,
    pub process_cpu_percent: f32,
    pub process_mem_used_bytes: u64,
    pub network_rx_bytes_per_sec: f64,
    pub network_tx_bytes_per_sec: f64,
    pub disk_io_bytes_per_sec: f64,
}

impl From<&ResourceTelemetry> for ResourceSample {
    fn from(sample: &ResourceTelemetry) -> Self {
        Self {
            host_cpu_percent: sample.cpu_percent,
            host_mem_percent: sample.mem_percent,
            host_mem_used_bytes: sample.mem_used_bytes,
            host_mem_total_bytes: sample.mem_total_bytes,
            process_cpu_percent: sample.process.cpu_percent,
            process_mem_used_bytes: sample.process.mem_used_bytes,
            network_rx_bytes_per_sec: sample.network.rx_bytes_per_sec,
            network_tx_bytes_per_sec: sample.network.tx_bytes_per_sec,
            disk_io_bytes_per_sec: sample
                .disks
                .iter()
                .map(|disk| disk.read_bytes_per_sec + disk.written_bytes_per_sec)
                .sum(),
        }
    }
}
