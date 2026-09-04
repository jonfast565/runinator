//! Published workflow contract compatibility and direct consumer impact.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{semver::SemVer, types::RuninatorType, workflows::WorkflowDefinition};

/// Legacy wire snapshots stored declarations only in graph metadata. Explicit Any wins.
pub(crate) fn with_legacy_output_type(mut value: serde_json::Value) -> serde_json::Value {
    if value.get("output_type").is_none()
        && let Some(output) = value
            .pointer("/definition/metadata/rexrap/output_type")
            .cloned()
        && let Some(object) = value.as_object_mut()
    {
        object.insert("output_type".into(), output);
    }
    value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContractCompatibility {
    Unchanged,
    Compatible,
    Breaking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDependent {
    pub kind: String,
    pub id: Uuid,
    pub name: String,
    /// Pinned consumers remain on their immutable revision.
    pub pinned: bool,
}

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

/// Conservative structural subtyping, including open-record and optional-field constraints.
pub fn contract_assignable(actual: &RuninatorType, expected: &RuninatorType) -> bool {
    use RuninatorType::*;
    if actual == expected || matches!(expected, Any) {
        return true;
    }
    match (actual, expected) {
        (Union(types), expected) => types.iter().all(|ty| contract_assignable(ty, expected)),
        (actual, Union(types)) => types.iter().any(|ty| contract_assignable(actual, ty)),
        (Array(a), Array(b)) | (Map(a), Map(b)) => contract_assignable(a, b),
        (
            Struct {
                fields: a,
                additional: extra_a,
            },
            Struct {
                fields: b,
                additional: extra_b,
            },
        ) => {
            for (key, field) in b {
                match a.get(key) {
                    Some(value)
                        if (!field.required || value.required)
                            && contract_assignable(&value.ty, &field.ty) => {}
                    None if !field.required
                        && extra_a
                            .as_ref()
                            .is_none_or(|ty| contract_assignable(ty, &field.ty)) => {}
                    _ => return false,
                }
            }
            a.iter().all(|(key, field)| {
                b.contains_key(key)
                    || extra_b
                        .as_ref()
                        .is_some_and(|ty| contract_assignable(&field.ty, ty))
            }) && extra_a
                .as_ref()
                .is_none_or(|a| extra_b.as_ref().is_some_and(|b| contract_assignable(a, b)))
        }
        (Map(a), Struct { fields, additional }) => {
            additional
                .as_ref()
                .is_some_and(|b| contract_assignable(a, b))
                && fields
                    .values()
                    .all(|f| !f.required && contract_assignable(a, &f.ty))
        }
        (Struct { fields, additional }, Map(b)) => {
            fields.values().all(|f| contract_assignable(&f.ty, b))
                && additional
                    .as_ref()
                    .is_none_or(|a| contract_assignable(a, b))
        }
        (Function { params: a, ret: ar }, Function { params: b, ret: br }) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| contract_assignable(b, a))
                && contract_assignable(ar, br)
        }
        _ => actual.validate_assignable_to(expected).is_ok(),
    }
}

#[cfg(test)]
#[path = "workflow_contracts_tests.rs"]
mod tests;
