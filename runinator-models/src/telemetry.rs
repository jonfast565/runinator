// resource-usage telemetry sampled by each replica and carried on heartbeats under
// `attributes.telemetry`. kept transport-friendly so it round-trips through the replica
// registry and the `/replicas` API without bespoke mapping.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

mod resource_telemetry;
pub use resource_telemetry::ResourceTelemetry;

mod replica_sample;
pub use replica_sample::ReplicaSample;

mod replica_sample_series;
pub use replica_sample_series::ReplicaSampleSeries;

mod load_average;
pub use load_average::LoadAverage;

mod process_telemetry;
pub use process_telemetry::ProcessTelemetry;

mod disk_telemetry;
pub use disk_telemetry::DiskTelemetry;

mod network_telemetry;
pub use network_telemetry::NetworkTelemetry;

mod host_metadata;
pub use host_metadata::HostMetadata;

mod gpu_telemetry;
pub use gpu_telemetry::GpuTelemetry;
