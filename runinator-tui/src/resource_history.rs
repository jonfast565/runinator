#[allow(unused_imports)]
use super::*;

/// The recent host-resource values rendered by the dashboard. The TUI owns this short window so
/// it can graph a local process without depending on a web service or durable store.
#[derive(Default)]
pub(super) struct ResourceHistory {
    pub(super) host_cpu: Vec<u64>,
    pub(super) host_memory: Vec<u64>,
    pub(super) host_memory_used: u64,
    pub(super) host_memory_total: u64,
    pub(super) process_cpu: Vec<u64>,
    pub(super) network_rx: Vec<u64>,
    pub(super) network_tx: Vec<u64>,
    pub(super) disk_io: Vec<u64>,
}

impl ResourceHistory {
    pub(super) fn push(&mut self, resources: &runinator_models::telemetry::ResourceTelemetry) {
        push_history(&mut self.host_cpu, percent(resources.cpu_percent));
        push_history(&mut self.host_memory, percent(resources.mem_percent));
        self.host_memory_used = resources.mem_used_bytes;
        self.host_memory_total = resources.mem_total_bytes;
        push_history(
            &mut self.process_cpu,
            percent(resources.process.cpu_percent),
        );
        push_history(
            &mut self.network_rx,
            rate(resources.network.rx_bytes_per_sec),
        );
        push_history(
            &mut self.network_tx,
            rate(resources.network.tx_bytes_per_sec),
        );
        push_history(
            &mut self.disk_io,
            rate(
                resources
                    .disks
                    .iter()
                    .map(|disk| disk.read_bytes_per_sec + disk.written_bytes_per_sec)
                    .sum(),
            ),
        );
    }
}
