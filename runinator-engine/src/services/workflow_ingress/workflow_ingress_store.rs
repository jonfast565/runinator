#[allow(unused_imports)]
use super::*;

pub trait WorkflowIngressStore:
    RuntimeStore
    + WorkflowVmStore
    + RunStore
    + ScheduleStore
    + FileStore
    + AuthStore
    + RbacStore
    + IngressStore
    + OrchestrationStore
{
}

impl<T> WorkflowIngressStore for T where
    T: RuntimeStore
        + WorkflowVmStore
        + RunStore
        + ScheduleStore
        + FileStore
        + AuthStore
        + RbacStore
        + IngressStore
        + OrchestrationStore
{
}
