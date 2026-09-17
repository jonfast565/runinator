#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OrchestrationPolicy {
    #[serde(default)]
    pub intents: BTreeMap<String, IntentPolicy>,
    #[serde(default)]
    pub phases: BTreeMap<String, PhasePolicy>,
    #[serde(default)]
    pub budgets: BTreeMap<String, BudgetPolicy>,
    /// The member used for the first epoch. Subsequent routing can only select declared phases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_member: Option<String>,
    /// An explicit cap on epochs created by outcome routes. This prevents an agent-produced route
    /// from creating an unbounded loop when a mission author forgot a terminal outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_epochs: Option<u32>,
    #[serde(default)]
    pub defaults: Value,
}

impl OrchestrationPolicy {
    pub fn validate<'a>(
        &self,
        member_keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), String> {
        let member_keys = member_keys
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let mut priorities = std::collections::BTreeMap::new();
        for (name, intent) in &self.intents {
            if name.trim().is_empty() {
                return Err("orchestration intent names must not be empty".into());
            }
            if let Some(existing) = priorities.insert(intent.priority, name) {
                return Err(format!(
                    "orchestration intents '{existing}' and '{name}' use the same priority {}",
                    intent.priority
                ));
            }
            if let RestartSelector::Member(member) = &intent.restart
                && !member_keys.contains(member.as_str())
            {
                return Err(format!(
                    "orchestration restart member '{member}' does not exist"
                ));
            }
            if let Some(pointer) = &intent.subject_revision_pointer {
                validate_json_pointer(pointer)?;
            }
        }
        for (member, phase) in &self.phases {
            if !member_keys.contains(member.as_str()) {
                return Err(format!(
                    "orchestration phase member '{member}' does not exist"
                ));
            }
            for pointer in [
                &phase.result.subject_revision,
                &phase.result.resources,
                &phase.result.evidence,
                &phase.result.failure_class,
                &phase.result.correlations,
                &phase.result.resources_patch,
                &phase.result.next_member,
            ]
            .into_iter()
            .flatten()
            {
                validate_json_pointer(pointer)?;
            }
            if let Some(workspace) = &phase.workspace
                && workspace.scope.trim().is_empty()
            {
                return Err(format!(
                    "workspace scope for phase '{member}' must not be empty"
                ));
            }
        }
        for (name, budget) in &self.budgets {
            if name.trim().is_empty() || budget.attempts == 0 {
                return Err("budget names must be non-empty and attempts must be positive".into());
            }
            if let Some(member) = &budget.handoff
                && !member_keys.contains(member.as_str())
            {
                return Err(format!(
                    "orchestration budget handoff member '{member}' does not exist"
                ));
            }
        }
        if self.max_epochs == Some(0) {
            return Err("orchestration max_epochs must be positive when supplied".into());
        }
        if let Some(member) = &self.entry_member
            && !member_keys.contains(member.as_str())
        {
            return Err(format!(
                "orchestration entry member '{member}' does not exist"
            ));
        }
        Ok(())
    }
}
