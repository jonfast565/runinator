#[allow(unused_imports)]
use super::*;

/// a REXRAP test suite: named fixtures plus cases run against one compiled workflow (or, for
/// multi-workflow packs, the workflow each case names).
#[derive(Debug, Clone)]
pub struct WorkflowTestSuite {
    /// default workflow name for cases that do not name their own; optional for single-workflow packs.
    pub workflow: Option<String>,
    pub tests: Vec<WorkflowTestCase>,
}

impl<'de> Deserialize<'de> for WorkflowTestSuite {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireSuite {
            #[serde(default)]
            workflow: Option<String>,
            #[serde(default)]
            fixtures: std::collections::BTreeMap<String, serde_json::Value>,
            tests: Vec<serde_json::Value>,
        }

        use serde::de::Error as _;

        let wire = WireSuite::deserialize(deserializer)?;
        let mut tests = Vec::with_capacity(wire.tests.len());
        for mut case in wire.tests {
            let fixture = case
                .as_object_mut()
                .and_then(|object| object.remove("fixture"));
            let resolved = match fixture {
                None => case,
                Some(serde_json::Value::String(name)) => {
                    let mut base = wire.fixtures.get(&name).cloned().ok_or_else(|| {
                        D::Error::custom(format!("test case references unknown fixture '{name}'"))
                    })?;
                    merge_override(&mut base, case);
                    base
                }
                Some(_) => {
                    return Err(D::Error::custom(
                        "test case `fixture` must name a fixture string",
                    ));
                }
            };
            tests.push(serde_json::from_value(resolved).map_err(D::Error::custom)?);
        }
        Ok(Self {
            workflow: wire.workflow,
            tests,
        })
    }
}

fn merge_override(base: &mut serde_json::Value, override_value: serde_json::Value) {
    match (base, override_value) {
        (serde_json::Value::Object(base), serde_json::Value::Object(overrides)) => {
            for (key, value) in overrides {
                if let Some(existing) = base.get_mut(&key) {
                    merge_override(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (base, value) => *base = value,
    }
}
