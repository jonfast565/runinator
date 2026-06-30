use std::path::PathBuf;

use runinator_provisioner::{
    KubernetesBackendConfig, ProvisionerConfig, SupervisorBackendConfig, SupervisorNodeTemplate,
};
use runinator_utilities::app_data;

// reads the on-demand node provisioner configuration from the environment. each backend is opt-in;
// an all-disabled environment yields an empty config (provisioning endpoints report no backends).
pub(crate) fn from_env() -> ProvisionerConfig {
    ProvisionerConfig {
        supervisor: supervisor_backend_from_env(),
        kubernetes: kubernetes_backend_from_env(),
    }
}

fn env_enabled(key: &str) -> bool {
    std::env::var(key)
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes"))
        .unwrap_or(false)
}

fn supervisor_backend_from_env() -> Option<SupervisorBackendConfig> {
    if !env_enabled("RUNINATOR_PROVISIONER_SUPERVISOR_ENABLED") {
        return None;
    }
    let state_file = std::env::var("RUNINATOR_PROVISIONER_SUPERVISOR_STATE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            app_data::default_supervisor_state_dir()
                .map(|dir| dir.join("state.json"))
                .unwrap_or_else(|_| PathBuf::from("supervisor/state.json"))
        });
    // the control queue lives beside the state file (state_dir/control), matching the supervisor.
    let control_dir = state_file
        .parent()
        .map(|parent| parent.join("control"))
        .unwrap_or_else(|| PathBuf::from("supervisor/control"));

    Some(SupervisorBackendConfig {
        control_dir,
        state_file,
        worker_template: template_from_env("RUNINATOR_PROVISIONER_SUPERVISOR_WORKER"),
        waker_template: template_from_env("RUNINATOR_PROVISIONER_SUPERVISOR_WAKER"),
    })
}

fn template_from_env(key: &str) -> Option<SupervisorNodeTemplate> {
    let raw = std::env::var(key).ok()?;
    match serde_json::from_str::<SupervisorNodeTemplate>(&raw) {
        Ok(template) => Some(template),
        Err(err) => {
            log::warn!("ignoring invalid {key} provisioner template: {err}");
            None
        }
    }
}

fn kubernetes_backend_from_env() -> Option<KubernetesBackendConfig> {
    if !env_enabled("RUNINATOR_PROVISIONER_K8S_ENABLED") {
        return None;
    }
    Some(KubernetesBackendConfig {
        namespace: std::env::var("RUNINATOR_PROVISIONER_K8S_NAMESPACE")
            .unwrap_or_else(|_| "runinator".to_string()),
        worker_deployment: Some(
            std::env::var("RUNINATOR_PROVISIONER_K8S_WORKER_DEPLOYMENT")
                .unwrap_or_else(|_| "runinator-worker".to_string()),
        ),
        waker_deployment: Some(
            std::env::var("RUNINATOR_PROVISIONER_K8S_WAKER_DEPLOYMENT")
                .unwrap_or_else(|_| "runinator-waker".to_string()),
        ),
    })
}
