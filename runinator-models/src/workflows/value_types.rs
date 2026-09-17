use super::*;

impl From<WorkflowObject> for Value {
    fn from(value: WorkflowObject) -> Self {
        value.into_value()
    }
}

impl From<WorkflowCondition> for Value {
    fn from(value: WorkflowCondition) -> Self {
        value.to_value()
    }
}

mod workflow_object;
pub use workflow_object::WorkflowObject;

mod workflow_condition;
pub use workflow_condition::WorkflowCondition;
