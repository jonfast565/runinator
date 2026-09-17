#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionProfileBinding {
    pub reference: ArtifactRef,
}

impl ExecutionProfileBinding {
    pub fn resolved(id: Uuid, name: impl Into<String>) -> Self {
        Self {
            reference: ArtifactRef::current(
                ArtifactKind::ExecutionProfile,
                id,
                Some(ArtifactPath::new(None, name)),
            ),
        }
    }

    pub fn unresolved(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            reference: ArtifactRef::current(
                ArtifactKind::ExecutionProfile,
                Uuid::nil(),
                Some(ArtifactPath::new(None, name)),
            ),
        }
    }

    pub fn id(&self) -> Uuid {
        self.reference.id
    }

    pub fn name(&self) -> &str {
        self.reference
            .authored_path
            .as_ref()
            .map(|path| path.key.as_str())
            .unwrap_or("")
    }
}

impl<'de> Deserialize<'de> for ExecutionProfileBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Current { reference: ArtifactRef },
            Legacy { id: Uuid, name: String },
        }

        use serde::de::Error as _;

        Ok(match Wire::deserialize(deserializer)? {
            Wire::Current { reference } => {
                if reference.kind != ArtifactKind::ExecutionProfile {
                    return Err(D::Error::custom(
                        "execution profile binding must reference an execution_profile artifact",
                    ));
                }
                Self { reference }
            }
            Wire::Legacy { id, name } => Self {
                reference: ArtifactRef::current(
                    ArtifactKind::ExecutionProfile,
                    id,
                    Some(ArtifactPath::new(None, name)),
                ),
            },
        })
    }
}
