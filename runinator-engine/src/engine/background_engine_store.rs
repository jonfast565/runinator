#[allow(unused_imports)]
use super::*;

pub trait BackgroundEngineStore:
    RuntimeStore
    + WorkflowVmStore
    + RunStore
    + runinator_store::roles::AiUsageStore
    + runinator_store::roles::FileStore
    + NotificationStore
    + ReplicaStore
    + OrgStore
    + ScheduleStore
    + DefinitionStore
    + IngressStore
    + WorkspaceStore
    + runinator_store::roles::DurableWorkspaceStore
    + OrchestrationStore
    + SettingStore
    + DeliveryStore
    + RbacStore
    + AuthStore
{
}

impl<T> BackgroundEngineStore for T where
    T: RuntimeStore
        + WorkflowVmStore
        + RunStore
        + runinator_store::roles::AiUsageStore
        + runinator_store::roles::FileStore
        + NotificationStore
        + ReplicaStore
        + OrgStore
        + ScheduleStore
        + DefinitionStore
        + IngressStore
        + WorkspaceStore
        + runinator_store::roles::DurableWorkspaceStore
        + OrchestrationStore
        + SettingStore
        + DeliveryStore
        + RbacStore
        + AuthStore
{
}
