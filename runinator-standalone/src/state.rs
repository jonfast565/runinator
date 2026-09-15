use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use chrono::Utc;
use runinator_models::local_runtime::{
    LocalDashboardSnapshot, LocalResourceSample, LocalRuntimeComponentSnapshot,
    LocalRuntimeHostKind, LocalRuntimeSnapshot,
};

#[derive(Clone)]
pub struct StateTracker {
    started_at: String,
    components: Arc<Mutex<BTreeMap<String, Component>>>,
    resources: Arc<runinator_observability::resource_telemetry::TelemetryCollector>,
}

struct Component {
    kind: String,
    status: String,
    restarts: u32,
    started: Option<Instant>,
    last_error: Option<String>,
}

impl StateTracker {
    pub fn new() -> Self {
        Self {
            started_at: Utc::now().to_rfc3339(),
            components: Arc::new(Mutex::new(BTreeMap::new())),
            resources: Arc::new(
                runinator_observability::resource_telemetry::TelemetryCollector::new(),
            ),
        }
    }

    pub fn starting(&self, id: &str, kind: &str) {
        {
            let mut components = self.components.lock().unwrap_or_else(|p| p.into_inner());
            let component = components
                .entry(id.to_string())
                .or_insert_with(|| Component {
                    kind: kind.to_string(),
                    status: "starting".into(),
                    restarts: 0,
                    started: None,
                    last_error: None,
                });
            component.kind = kind.to_string();
            component.status = "starting".into();
            component.started = None;
            component.last_error = None;
        }
        self.publish(kind, id, "starting");
    }

    pub fn running(&self, id: &str) {
        let kind = {
            let mut components = self.components.lock().unwrap_or_else(|p| p.into_inner());
            let Some(component) = components.get_mut(id) else {
                return;
            };
            component.status = "running".into();
            component.started.get_or_insert_with(Instant::now);
            component.last_error = None;
            component.kind.clone()
        };
        self.publish(&kind, id, "running");
    }

    pub fn stopped(&self, id: &str) {
        self.set_status(id, "stopped", None);
    }

    pub fn failed(&self, id: &str, error: impl ToString) {
        self.set_status(id, "failed", Some(error.to_string()));
    }

    pub fn restarting(&self, id: &str) {
        let kind = {
            let mut components = self.components.lock().unwrap_or_else(|p| p.into_inner());
            let Some(component) = components.get_mut(id) else {
                return;
            };
            component.status = "backoff".into();
            component.restarts += 1;
            component.kind.clone()
        };
        self.publish(&kind, id, "backoff");
    }

    pub fn status(&self, id: &str) -> Option<String> {
        self.components
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .map(|component| component.status.clone())
    }

    fn set_status(&self, id: &str, status: &str, error: Option<String>) {
        let kind = {
            let mut components = self.components.lock().unwrap_or_else(|p| p.into_inner());
            let Some(component) = components.get_mut(id) else {
                return;
            };
            component.status = status.into();
            component.last_error = error;
            component.kind.clone()
        };
        self.publish(&kind, id, status);
    }

    fn publish(&self, kind: &str, id: &str, status: &str) {
        crate::dashboard::publish_transition(kind, id, status);
        crate::dashboard::publish_snapshot(&self.snapshot());
    }

    pub fn snapshot(&self) -> LocalRuntimeSnapshot {
        let components = self.components.lock().unwrap_or_else(|p| p.into_inner());
        LocalRuntimeSnapshot {
            host_kind: LocalRuntimeHostKind::Standalone,
            pid: std::process::id(),
            started_at: self.started_at.clone(),
            updated_at: Utc::now().to_rfc3339(),
            components: components
                .iter()
                .map(|(id, component)| LocalRuntimeComponentSnapshot {
                    id: id.clone(),
                    kind: component.kind.clone(),
                    status: component.status.clone(),
                    restarts: component.restarts,
                    uptime_seconds: component.started.map(|started| started.elapsed().as_secs()),
                    last_error: component.last_error.clone(),
                })
                .collect(),
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let snapshot = self.snapshot();
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&snapshot)?)?;
        replace_file(&temp, path)?;
        let Some(state_dir) = path.parent() else {
            return Ok(());
        };
        let resource = self.resources.sample();
        let dashboard = LocalDashboardSnapshot {
            version: 1,
            host_id: runinator_observability::resource_telemetry::host_metadata()
                .host_name
                .unwrap_or_else(|| format!("standalone-{}", std::process::id())),
            host_kind: snapshot.host_kind,
            pid: snapshot.pid,
            started_at: snapshot.started_at,
            updated_at: snapshot.updated_at,
            resource_samples: vec![LocalResourceSample {
                sampled_at: resource.sampled_at.to_rfc3339(),
                cpu_percent: f64::from(resource.cpu_percent),
                memory_bytes: resource.mem_used_bytes,
            }],
            components: snapshot.components,
            log_location: state_dir.join("standalone.log").display().to_string(),
        };
        let dashboard_path = state_dir.join("dashboard.json");
        let dashboard_temp = state_dir.join("dashboard.json.tmp");
        std::fs::write(&dashboard_temp, serde_json::to_vec_pretty(&dashboard)?)?;
        replace_file(&dashboard_temp, &dashboard_path)?;
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
