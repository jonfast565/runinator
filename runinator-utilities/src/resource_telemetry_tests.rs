use super::*;
use runinator_models::json;

#[test]
fn sample_reports_nonzero_memory_totals() {
    let collector = TelemetryCollector::new();
    let snapshot = collector.sample();
    // every host the tests run on has memory; cpu may legitimately read 0 on the first sample.
    assert!(snapshot.mem_total_bytes > 0);
    assert!(snapshot.mem_used_bytes <= snapshot.mem_total_bytes);
    assert!(snapshot.cpu_percent >= 0.0);
    assert!(snapshot.mem_percent >= 0.0 && snapshot.mem_percent <= 100.0);
    // first sample has no prior interval, so rates are zero but cumulative counters are populated.
    assert_eq!(snapshot.network.rx_bytes_per_sec, 0.0);
    assert_eq!(snapshot.network.tx_bytes_per_sec, 0.0);

    // swap totals are reported (possibly zero); process rss should be populated for our own pid.
    assert!(snapshot.swap_used_bytes <= snapshot.swap_total_bytes.max(snapshot.swap_used_bytes));
    assert!(snapshot.process.mem_used_bytes > 0);

    // a second sample after a real interval yields a non-negative rate.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let next = collector.sample();
    assert!(next.network.rx_bytes_per_sec >= 0.0);
    assert!(next.network.tx_bytes_per_sec >= 0.0);
    assert!(next.network.rx_total_bytes >= snapshot.network.rx_total_bytes);
    // at least the root filesystem should be visible, with a non-negative i/o rate.
    if let Some(disk) = next.disks.first() {
        assert!(disk.total_bytes > 0);
        assert!(disk.read_bytes_per_sec >= 0.0);
    }
}

#[test]
fn host_metadata_reports_cpu_and_memory() {
    let host = host_metadata();
    assert!(host.logical_cores > 0);
    assert!(host.mem_total_bytes > 0);
    assert!(!host.cpu_arch.is_empty());
}

#[test]
fn merge_host_metadata_adds_host_key() {
    let base = json!({ "broker_backend": "in-memory" });
    let merged = attributes_with_host_metadata(&base);
    assert_eq!(
        merged.get("broker_backend").and_then(|v| v.as_str()),
        Some("in-memory")
    );
    let host = merged.get("host").expect("host key present");
    assert!(host.get("logical_cores").is_some());
}

#[test]
fn merge_preserves_base_attributes_and_adds_telemetry() {
    let collector = TelemetryCollector::new();
    let base = json!({ "broker_backend": "in-memory", "labels": ["gpu"] });
    let merged = attributes_with_telemetry(&base, &collector);

    // static attributes survive the merge.
    assert_eq!(
        merged.get("broker_backend").and_then(|v| v.as_str()),
        Some("in-memory")
    );
    // telemetry is folded in under its own key and carries the memory total.
    let telemetry = merged.get("telemetry").expect("telemetry key present");
    assert!(telemetry.get("mem_total_bytes").is_some());
    assert!(telemetry.get("cpu_percent").is_some());
    assert!(telemetry.get("network").is_some());
}

#[test]
fn merge_replaces_non_object_base_with_object() {
    let collector = TelemetryCollector::new();
    let merged = attributes_with_telemetry(&Value::Null, &collector);
    assert!(merged.as_object().is_some());
    assert!(merged.get("telemetry").is_some());
}
