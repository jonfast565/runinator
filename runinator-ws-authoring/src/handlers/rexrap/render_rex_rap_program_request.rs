#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct RenderRexRapProgramRequest {
    pub program: Value,
}

impl Validate for RenderRexRapProgramRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        dynamic_value("program", &self.program)?;
        if !self.program.is_array() {
            return Err(ValidationError::new("program", "must be an array"));
        }
        Ok(())
    }
}
