#[allow(unused_imports)]
use super::*;

pub trait RunOperationsStore:
    AuthorizationStore
    + RuntimeStore
    + WorkflowVmStore
    + RunStore
    + ScheduleStore
    + FileStore
    + IngressStore
    + OrchestrationStore
    + AiUsageStore
{
}

impl<T> RunOperationsStore for T where
    T: AuthorizationStore
        + RuntimeStore
        + WorkflowVmStore
        + RunStore
        + ScheduleStore
        + FileStore
        + IngressStore
        + OrchestrationStore
        + AiUsageStore
{
}
