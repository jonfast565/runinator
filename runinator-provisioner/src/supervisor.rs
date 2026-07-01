use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use runinator_models::errors::SendableError;
use runinator_models::provisioning::{NodeSpec, ProvisionBackend, ProvisionedGroup};
use runinator_models::replicas::ReplicaKind;
use runinator_supervisor::config::ProcessConfig;
use runinator_supervisor::control::{enqueue, ControlCommand};
use runinator_supervisor::snapshot::read_snapshot;
use uuid::Uuid;

use crate::errors::{ENQUEUE_FAILED, SNAPSHOT_READ, UNSUPPORTED_KIND};
use crate::traits::Provisioner;

// statuses the supervisor reports for a process that is up (or coming up).
const LIVE_STATUSES: &[&str] = &["running", "starting"];

/// process template used to spawn one node of a kind via the supervisor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SupervisorNodeTemplate {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// provisions worker/waker nodes as dynamic supervisor processes.
pub struct SupervisorProvisioner {
    control_dir: PathBuf,
    state_file: PathBuf,
    templates: BTreeMap<&'static str, SupervisorNodeTemplate>,
}

impl SupervisorProvisioner {
    pub fn new(control_dir: PathBuf, state_file: PathBuf) -> Self {
        Self {
            control_dir,
            state_file,
            templates: BTreeMap::new(),
        }
    }

    /// register the spawn template for a node kind. only kinds with a template are manageable.
    pub fn with_template(mut self, kind: ReplicaKind, template: SupervisorNodeTemplate) -> Self {
        self.templates.insert(kind.as_str(), template);
        self
    }

    fn template(&self, kind: ReplicaKind) -> Result<&SupervisorNodeTemplate, SendableError> {
        self.templates
            .get(kind.as_str())
            .ok_or_else(|| UNSUPPORTED_KIND.error(kind.as_str()))
    }

    // names of provisioned processes under a group prefix, paired with whether each is currently up.
    fn current_nodes(&self, prefix: &str) -> Result<Vec<(String, bool)>, SendableError> {
        if !self.state_file.exists() {
            return Ok(Vec::new());
        }
        let snapshot = read_snapshot(&self.state_file).map_err(|err| SNAPSHOT_READ.error(err))?;
        let mut nodes: Vec<(String, bool)> = snapshot
            .processes
            .into_iter()
            .filter(|p| p.name.starts_with(prefix))
            .map(|p| (p.name, LIVE_STATUSES.contains(&p.status.as_str())))
            .collect();
        nodes.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(nodes)
    }

    fn build_process(
        &self,
        kind: ReplicaKind,
        node_id: &str,
        spec: &NodeSpec,
    ) -> Result<ProcessConfig, SendableError> {
        let template = self.template(kind)?;
        let mut args = template.args.clone();
        args.push(id_flag(kind).to_string());
        args.push(node_id.to_string());
        args.extend(spec.extra_args.iter().cloned());

        let mut env = template.env.clone();
        if kind == ReplicaKind::Worker && !spec.labels.is_empty() {
            // workers read routing labels from this env var (comma-separated key=value).
            let labels = spec
                .labels
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",");
            env.insert("RUNINATOR_WORKER_LABELS".to_string(), labels);
        }

        Ok(ProcessConfig {
            name: node_id.to_string(),
            command: template.command.clone(),
            args,
            cwd: template.cwd.clone(),
            env,
            autostart: true,
            restart_on_failure: true,
            max_restarts_per_minute: 10,
        })
    }

    fn enqueue(&self, command: &ControlCommand) -> Result<(), SendableError> {
        enqueue(&self.control_dir, command).map_err(|err| ENQUEUE_FAILED.error(err))
    }

    fn group(
        &self,
        kind: ReplicaKind,
        group: &str,
        desired: u32,
        available: u32,
    ) -> ProvisionedGroup {
        ProvisionedGroup {
            backend: ProvisionBackend::Supervisor,
            kind,
            name: group.to_string(),
            desired,
            available,
            manageable: self.templates.contains_key(kind.as_str()),
        }
    }
}

// the effective group for a scale request: an explicit `spec.group` (e.g. a per-org pool) or the
// kind's default group. the kind default keeps pre-group behavior byte-compatible.
fn effective_group(kind: ReplicaKind, spec: &NodeSpec) -> String {
    spec.group
        .clone()
        .unwrap_or_else(|| kind.as_str().to_string())
}

// the process-name prefix that marks processes as provisioner-managed for a group.
fn prefix_for(group: &str) -> String {
    format!("prov-{group}-")
}

#[async_trait]
impl Provisioner for SupervisorProvisioner {
    fn backend(&self) -> ProvisionBackend {
        ProvisionBackend::Supervisor
    }

    fn supported_kinds(&self) -> Vec<ReplicaKind> {
        let mut kinds = Vec::new();
        for kind in [ReplicaKind::Worker, ReplicaKind::Waker] {
            if self.templates.contains_key(kind.as_str()) {
                kinds.push(kind);
            }
        }
        kinds
    }

    async fn available(&self) -> bool {
        // the supervisor is reachable when its state snapshot exists.
        self.state_file.exists()
    }

    async fn list(&self) -> Result<Vec<ProvisionedGroup>, SendableError> {
        // list reports each kind's default group; per-group (e.g. per-org) pools are tracked by the
        // caller (the web service records org allocations) and addressed via `spec.group` on scale.
        let mut groups = Vec::new();
        for kind in self.supported_kinds() {
            let group = kind.as_str().to_string();
            let nodes = self.current_nodes(&prefix_for(&group))?;
            let available = nodes.iter().filter(|(_, up)| *up).count() as u32;
            groups.push(self.group(kind, &group, nodes.len() as u32, available));
        }
        Ok(groups)
    }

    async fn scale(
        &self,
        kind: ReplicaKind,
        desired: u32,
        spec: &NodeSpec,
    ) -> Result<ProvisionedGroup, SendableError> {
        self.template(kind)?;
        let group = effective_group(kind, spec);
        let prefix = prefix_for(&group);
        let mut current = self.current_nodes(&prefix)?;
        let current_count = current.len() as u32;

        if desired > current_count {
            for _ in 0..(desired - current_count) {
                let node_id = format!("{prefix}{}", Uuid::new_v4());
                let process = self.build_process(kind, &node_id, spec)?;
                self.enqueue(&ControlCommand::AddProcess { process })?;
            }
        } else if desired < current_count {
            // remove the newest (highest-sorted) provisioned nodes first.
            current.sort_by(|a, b| a.0.cmp(&b.0));
            let remove_count = (current_count - desired) as usize;
            for (name, _) in current.iter().rev().take(remove_count) {
                self.enqueue(&ControlCommand::RemoveProcess { name: name.clone() })?;
            }
        }

        let available = current.iter().filter(|(_, up)| *up).count() as u32;
        Ok(self.group(kind, &group, desired, available.min(desired)))
    }

    async fn stop(&self, node_id: &str) -> Result<(), SendableError> {
        self.enqueue(&ControlCommand::RemoveProcess {
            name: node_id.to_string(),
        })
    }
}

// the cli flag a node binary uses to receive its generated id.
fn id_flag(kind: ReplicaKind) -> &'static str {
    match kind {
        ReplicaKind::Worker => "--worker-id",
        ReplicaKind::Waker => "--waker-id",
        ReplicaKind::Webservice => "--instance-id",
    }
}
