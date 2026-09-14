use std::{collections::BTreeMap, thread::JoinHandle};

use runinator_models::local_runtime::LocalRuntimeSnapshot;
use runinator_platform::startup::Shutdown;

use crate::config::StandaloneConfig;

const STANDALONE: &str = "standalone";
const WEB_SERVICE: &str = "web service";
const BROKER: &str = "broker";
const BLOB: &str = "blob";
const ADAPTER_HOST: &str = "adapter host";
const ENGINE: &str = "engine";
const WORKER: &str = "worker";
const WAKER: &str = "waker";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RoleSummary {
    configured: i64,
    running: i64,
    starting: i64,
    degraded: i64,
    restarts: i64,
}

pub fn start(config: &StandaloneConfig, shutdown: Shutdown) -> JoinHandle<()> {
    let dashboard = runinator_observability::tui::install();
    register_components(config);
    runinator_observability::tui::activity(STANDALONE, "starting local runtime", None);
    let dashboard_shutdown = shutdown.clone();
    runinator_observability::tui::spawn(
        dashboard,
        move || dashboard_shutdown.is_cancelled(),
        move || shutdown.trigger(),
    )
}

pub fn publish_transition(kind: &str, id: &str, status: &str) {
    let component = component_name(kind);
    let activity = format!("{id} {status}");
    runinator_observability::tui::activity(component, activity.clone(), None);
    if component != STANDALONE {
        runinator_observability::tui::activity(STANDALONE, activity, None);
    }
}

pub fn publish_snapshot(snapshot: &LocalRuntimeSnapshot) {
    for (component, summary) in summarize(snapshot) {
        runinator_observability::tui::gauge(component, "configured", summary.configured);
        runinator_observability::tui::gauge(component, "running", summary.running);
        runinator_observability::tui::gauge(component, "starting", summary.starting);
        runinator_observability::tui::gauge(component, "degraded", summary.degraded);
        runinator_observability::tui::gauge(component, "restarts", summary.restarts);
    }
}

fn register_components(config: &StandaloneConfig) {
    let components = [
        (
            STANDALONE,
            vec![
                format!("database {}", config.database),
                format!("state {}", config.state_dir.display()),
            ],
        ),
        (
            WEB_SERVICE,
            vec![format!("http://127.0.0.1:{}", config.api_port)],
        ),
        (
            BROKER,
            vec![format!("tcp://127.0.0.1:{}", config.broker_port)],
        ),
        (BLOB, vec![format!("http://127.0.0.1:{}", config.blob_port)]),
        (
            ADAPTER_HOST,
            vec![format!("http://127.0.0.1:{}", config.adapter_port)],
        ),
        (
            ENGINE,
            vec![
                format!("configured replicas {}", config.engines),
                format!("ingress concurrency {}", config.engine_concurrency),
            ],
        ),
        (
            WORKER,
            vec![
                format!(
                    "configured replicas {}",
                    config.workers + u32::from(config.desktop_agent)
                ),
                format!("max actions {}", config.worker_concurrency),
            ],
        ),
        (
            WAKER,
            vec![format!("configured replicas {}", config.wakers)],
        ),
    ];
    for (component, details) in components {
        runinator_observability::tui::register(component, details);
    }
}

fn summarize(snapshot: &LocalRuntimeSnapshot) -> BTreeMap<&'static str, RoleSummary> {
    let mut summaries: BTreeMap<&'static str, RoleSummary> = [
        STANDALONE,
        WEB_SERVICE,
        BROKER,
        BLOB,
        ADAPTER_HOST,
        ENGINE,
        WORKER,
        WAKER,
    ]
    .into_iter()
    .map(|component| (component, RoleSummary::default()))
    .collect();
    for component in &snapshot.components {
        if component.status == "stopped" {
            continue;
        }
        let summary = summaries
            .entry(component_name(&component.kind))
            .or_default();
        summary.configured += 1;
        summary.restarts += i64::from(component.restarts);
        match component.status.as_str() {
            "running" => summary.running += 1,
            "starting" => summary.starting += 1,
            "failed" | "backoff" => summary.degraded += 1,
            _ => {}
        }
    }

    let mut total = RoleSummary::default();
    for (component, summary) in &summaries {
        if *component == STANDALONE {
            continue;
        }
        total.configured += summary.configured;
        total.running += summary.running;
        total.starting += summary.starting;
        total.degraded += summary.degraded;
        total.restarts += summary.restarts;
    }
    let host = summaries.entry(STANDALONE).or_default();
    host.configured += total.configured;
    host.running += total.running;
    host.starting += total.starting;
    host.degraded += total.degraded;
    host.restarts += total.restarts;
    summaries
}

fn component_name(kind: &str) -> &'static str {
    match kind {
        "webservice" => WEB_SERVICE,
        "broker" => BROKER,
        "blob" => BLOB,
        "adapter" => ADAPTER_HOST,
        "background" => ENGINE,
        "worker" => WORKER,
        "waker" => WAKER,
        _ => STANDALONE,
    }
}

#[cfg(test)]
#[path = "dashboard_tests.rs"]
mod tests;
