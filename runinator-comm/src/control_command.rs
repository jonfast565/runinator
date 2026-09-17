#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCommand {
    pub workflow_run_id: Uuid,
    pub kind: ControlKind,
    /// when set, the control applies to a single node run rather than the whole run. used to cancel
    /// an already-dispatched losing race branch without disturbing the winner or sibling work.
    /// defaults to `None` for backward-compatible deserialization of run-wide commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node_run_id: Option<Uuid>,
    /// VM execution target. Mutually exclusive with `workflow_node_run_id`; when present the
    /// control reaches exactly the provider effect identified here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<Uuid>,
    /// runtime routing key selecting which worker(s) should receive this control. the web service
    /// stamps the executing worker's replica (from the node run's executor claim) on cancels so
    /// they reach the holder instead of a random control consumer; `Any` (the default, and the
    /// deserialization of older messages) preserves the untargeted competing-consumer behavior.
    #[serde(default)]
    pub target: ActionTarget,
    /// Present only when `kind` is `terminal`. Kept separate from `kind` so the established
    /// cancel/pause/resume representation remains backward compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<ProviderTerminalControl>,
}

impl ControlCommand {
    pub fn new(workflow_run_id: Uuid, kind: ControlKind) -> Self {
        Self {
            workflow_run_id,
            kind,
            workflow_node_run_id: None,
            effect_id: None,
            target: ActionTarget::Any,
            terminal: None,
        }
    }

    /// a control targeting a single node run (e.g. cancelling one losing race branch).
    pub fn for_node_run(
        workflow_run_id: Uuid,
        workflow_node_run_id: Uuid,
        kind: ControlKind,
    ) -> Self {
        Self {
            workflow_run_id,
            kind,
            workflow_node_run_id: Some(workflow_node_run_id),
            effect_id: None,
            target: ActionTarget::Any,
            terminal: None,
        }
    }

    pub fn for_effect(workflow_run_id: Uuid, effect_id: Uuid, kind: ControlKind) -> Self {
        Self {
            workflow_run_id,
            kind,
            workflow_node_run_id: None,
            effect_id: Some(effect_id),
            target: ActionTarget::Any,
            terminal: None,
        }
    }

    pub fn for_terminal(
        workflow_run_id: Uuid,
        effect_id: Uuid,
        terminal: ProviderTerminalControl,
    ) -> Self {
        Self {
            workflow_run_id,
            kind: ControlKind::Terminal,
            workflow_node_run_id: None,
            effect_id: Some(effect_id),
            target: ActionTarget::Any,
            terminal: Some(terminal),
        }
    }

    /// route this control to the worker replica currently holding the executor lease, so it is not
    /// consumed (and dropped) by a worker that never dispatched the action.
    pub fn targeting_replica(mut self, replica_id: Uuid) -> Self {
        self.target = ActionTarget::Replica { replica_id };
        self
    }
}
