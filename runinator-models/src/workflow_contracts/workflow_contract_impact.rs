#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContractImpact {
    pub compatibility: ContractCompatibility,
    pub reasons: Vec<String>,
    pub previous_version: Option<SemVer>,
    pub proposed_version: SemVer,
    pub requires_major_bump: bool,
    pub dependents: Vec<ContractDependent>,
}

impl WorkflowContractImpact {
    pub fn compare(previous: Option<&WorkflowDefinition>, proposed: &WorkflowDefinition) -> Self {
        let mut impact = Self {
            compatibility: ContractCompatibility::Unchanged,
            reasons: Vec::new(),
            previous_version: previous.map(|w| w.version),
            proposed_version: proposed.version,
            requires_major_bump: false,
            dependents: Vec::new(),
        };
        let Some(previous) = previous else {
            return impact;
        };
        if previous.input_type == proposed.input_type
            && previous.output_type == proposed.output_type
        {
            return impact;
        }
        impact.compatibility = ContractCompatibility::Compatible;
        if !contract_assignable(&previous.input_type, &proposed.input_type) {
            impact.reasons.push(
                "input: the new contract does not accept every previously accepted input".into(),
            );
        }
        if !contract_assignable(&proposed.output_type, &previous.output_type) {
            impact.reasons.push(
                "output: the new contract does not guarantee the previous return shape".into(),
            );
        }
        if !impact.reasons.is_empty() {
            impact.compatibility = ContractCompatibility::Breaking;
            impact.requires_major_bump = proposed.version.major <= previous.version.major;
        }
        impact
    }
}
