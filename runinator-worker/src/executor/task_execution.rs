#[allow(unused_imports)]
use super::*;

pub(crate) struct TaskExecution {
    pub libraries: Arc<HashMap<String, Plugin>>,
    pub action: WorkflowAction,
    pub execution_id: Uuid,
    pub parameters: Value,
    pub idempotency_key: Option<String>,
    pub execution_profile: Option<MaterializedExecutionProfile>,
    pub sink: Option<Arc<dyn ProviderEventSink>>,
    pub token: CancellationToken,
}
