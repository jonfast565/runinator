use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use chrono::Utc;
use runinator_models::local_runtime::{
    LocalRuntimeComponentSnapshot, LocalRuntimeHostKind, LocalRuntimeSnapshot,
};

#[derive(Clone)]
pub struct StateTracker {
    started_at: String,
    components: Arc<Mutex<BTreeMap<String, Component>>>,
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
        }
    }

    pub fn starting(&self, id: &str, kind: &str) {
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

    pub fn running(&self, id: &str) {
        if let Some(component) = self
            .components
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(id)
        {
            component.status = "running".into();
            component.started.get_or_insert_with(Instant::now);
            component.last_error = None;
        }
    }

    pub fn stopped(&self, id: &str) {
        self.set_status(id, "stopped", None);
    }

    pub fn failed(&self, id: &str, error: impl ToString) {
        self.set_status(id, "failed", Some(error.to_string()));
    }

    pub fn restarting(&self, id: &str) {
        let mut components = self.components.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(component) = components.get_mut(id) {
            component.status = "backoff".into();
            component.restarts += 1;
        }
    }

    pub fn status(&self, id: &str) -> Option<String> {
        self.components
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .map(|component| component.status.clone())
    }

    fn set_status(&self, id: &str, status: &str, error: Option<String>) {
        if let Some(component) = self
            .components
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(id)
        {
            component.status = status.into();
            component.last_error = error;
        }
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
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&self.snapshot())?)?;
        std::fs::rename(temp, path)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
