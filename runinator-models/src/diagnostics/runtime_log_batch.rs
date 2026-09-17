#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLogBatch {
    pub records: Vec<RuntimeLogRecord>,
}

impl Validate for RuntimeLogBatch {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.records.len() > MAX_RUNTIME_LOG_BATCH {
            return Err(ValidationError::new(
                "records",
                format!("must contain at most {MAX_RUNTIME_LOG_BATCH} records"),
            ));
        }
        let mut bytes = 0usize;
        for (index, record) in self.records.iter().enumerate() {
            record.validate().map_err(|error| {
                ValidationError::new(format!("records.{index}.{}", error.path), error.message)
            })?;
            bytes = bytes.saturating_add(record.message.len());
        }
        if bytes > MAX_RUNTIME_LOG_BATCH_BYTES {
            return Err(ValidationError::new(
                "records",
                format!("messages must total at most {MAX_RUNTIME_LOG_BATCH_BYTES} bytes"),
            ));
        }
        Ok(())
    }
}
