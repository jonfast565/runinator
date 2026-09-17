use std::collections::BTreeMap;
use std::path::PathBuf;

use runinator_models::replicas::ReplicaKind;
use runinator_platform::{app_data, env};
use runinator_provisioner::{
    KubernetesBackendConfig, ProvisionerConfig, SupervisorBackendConfig, SupervisorNodeTemplate,
};

// reads the on-demand node provisioner configuration from the environment. each backend is opt-in;
// an all-disabled environment yields an empty config (provisioning endpoints report no backends).
pub(crate) fn from_env() -> ProvisionerConfig {
    ProvisionerConfig {
        supervisor: supervisor_backend_from_env(),
        kubernetes: kubernetes_backend_from_env(),
    }
}

fn supervisor_backend_from_env() -> Option<SupervisorBackendConfig> {
    if !env::flag("RUNINATOR_PROVISIONER_SUPERVISOR_ENABLED") {
        return None;
    }
    let state_file =
        env::path("RUNINATOR_PROVISIONER_SUPERVISOR_STATE_PATH").unwrap_or_else(|| {
            app_data::default_supervisor_state_dir()
                .map(|dir| dir.join("state.json"))
                .unwrap_or_else(|_| PathBuf::from("supervisor/state.json"))
        });
    // the control queue lives beside the state file (state_dir/control), matching the supervisor.
    let control_dir = state_file
        .parent()
        .map(|parent| parent.join("control"))
        .unwrap_or_else(|| PathBuf::from("supervisor/control"));

    // Read a spawn template per kind: RUNINATOR_PROVISIONER_SUPERVISOR_<KIND>. Iterating the
    // canonical kind list means a new kind gets an env slot automatically.
    let mut templates = BTreeMap::new();
    for &kind in ReplicaKind::ALL {
        let key = format!(
            "RUNINATOR_PROVISIONER_SUPERVISOR_{}",
            supervisor_suffix(kind)
        );
        if let Some(template) = template_from_env(&key) {
            templates.insert(kind, template);
        }
    }

    Some(SupervisorBackendConfig {
        control_dir,
        state_file,
        templates,
    })
}

// the env-var suffix naming a kind's supervisor template, e.g. WORKER / BACKGROUND.
fn supervisor_suffix(kind: ReplicaKind) -> String {
    kind.as_str().to_uppercase()
}

fn template_from_env(key: &str) -> Option<SupervisorNodeTemplate> {
    let raw = env::string(key)?;
    match serde_json::from_str::<SupervisorNodeTemplate>(&raw) {
        Ok(template) => Some(template),
        Err(err) => {
            log::warn!("ignoring invalid {key} provisioner template: {err}");
            None
        }
    }
}

fn kubernetes_backend_from_env() -> Option<KubernetesBackendConfig> {
    if !env::flag("RUNINATOR_PROVISIONER_K8S_ENABLED") {
        return None;
    }
    // Map a deployment per kind: RUNINATOR_PROVISIONER_K8S_<KIND>_DEPLOYMENT. Worker, waker, and
    // webservice keep their default names so existing deployments stay manageable out of the box;
    // other kinds are opt-in via their env var.
    let mut deployments = BTreeMap::new();
    for &kind in ReplicaKind::ALL {
        if kind == ReplicaKind::Postgres {
            continue; // postgres is a stateful set, handled below.
        }
        if let Some(name) = k8s_deployment(kind) {
            deployments.insert(kind, name);
        }
    }

    let mut stateful_sets = BTreeMap::new();
    if let Some(name) = env::non_empty("RUNINATOR_PROVISIONER_K8S_POSTGRES_STATEFULSET") {
        let name = name.trim().to_string();
        stateful_sets.insert(ReplicaKind::Postgres, name);
    }

    Some(KubernetesBackendConfig {
        namespace: env::non_empty("RUNINATOR_PROVISIONER_K8S_NAMESPACE")
            .unwrap_or_else(|| "runinator".to_string()),
        deployments,
        stateful_sets,
        postgres_scale_out_enabled: env::flag(
            "RUNINATOR_PROVISIONER_K8S_POSTGRES_SCALE_OUT_ENABLED",
        ),
    })
}

// the deployment name backing a kind: an explicit env override, else the default name for the
// original three kinds. other kinds are unmapped (ghost rows) until an env var is set.
fn k8s_deployment(kind: ReplicaKind) -> Option<String> {
    let infix = kind.as_str().to_uppercase();
    let key = format!("RUNINATOR_PROVISIONER_K8S_{infix}_DEPLOYMENT");
    if let Some(name) = env::non_empty(&key) {
        let name = name.trim().to_string();
        return Some(name);
    }
    match kind {
        ReplicaKind::Worker => Some("runinator-worker".to_string()),
        ReplicaKind::Waker => Some("runinator-waker".to_string()),
        ReplicaKind::Webservice => Some("runinator-ws".to_string()),
        _ => None,
    }
}
