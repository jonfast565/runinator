#[allow(unused_imports)]
use super::*;

pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

impl Validate for Value {
    fn validate(&self) -> Result<(), ValidationError> {
        dynamic_value("payload", self)
    }
}
