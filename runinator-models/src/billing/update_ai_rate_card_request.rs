#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAiRateCardRequest {
    #[serde(default)]
    pub ai_entries: Vec<AiRateEntry>,
}

impl Validate for UpdateAiRateCardRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.ai_entries.len() > 256 {
            return Err(ValidationError::new(
                "ai_entries",
                "must contain at most 256 entries",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for (index, entry) in self.ai_entries.iter().enumerate() {
            identifier(&format!("ai_entries.{index}.provider"), &entry.provider)?;
            if entry.model.trim().is_empty() || entry.model.len() > 200 {
                return Err(ValidationError::new(
                    format!("ai_entries.{index}.model"),
                    "must contain 1 to 200 characters",
                ));
            }
            if !seen.insert((entry.provider.clone(), entry.model.clone())) {
                return Err(ValidationError::new(
                    format!("ai_entries.{index}"),
                    "duplicates an existing provider/model entry",
                ));
            }
        }
        Ok(())
    }
}
