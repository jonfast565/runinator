//! Durable correlated orchestration bindings and their reducer outboxes.

use std::collections::BTreeMap;
use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::{
    errors::SendableError,
    orchestration::{
        AdapterAuthentication, AdapterDefinition, AdapterPollStatus, AdapterRevision,
        AdapterTransport, ExternalOperation, ExternalOperationStatus, NewOrchestrationBinding,
        OrchestrationBinding, OrchestrationCommand, OrchestrationCorrelationAlias,
        OrchestrationEpoch, OrchestrationEventReduction, OrchestrationEvidence,
        OrchestrationPendingIntent, OrchestrationStatus,
    },
    value::Value,
};
use uuid::Uuid;

/// Owns binding CAS, reducer leasing, immutable reductions/epochs, and the command outbox.
mod orchestration_binding_update;
pub use orchestration_binding_update::OrchestrationBindingUpdate;

mod orchestration_binding_filter;
pub use orchestration_binding_filter::OrchestrationBindingFilter;

mod new_orchestration_epoch;
pub use new_orchestration_epoch::NewOrchestrationEpoch;

mod new_orchestration_correlation_alias;
pub use new_orchestration_correlation_alias::NewOrchestrationCorrelationAlias;

mod new_orchestration_command;
pub use new_orchestration_command::NewOrchestrationCommand;

mod new_adapter_definition;
pub use new_adapter_definition::NewAdapterDefinition;

mod new_adapter_revision;
pub use new_adapter_revision::NewAdapterRevision;

mod adapter_poll_dispatch;
pub use adapter_poll_dispatch::AdapterPollDispatch;

mod external_operation_update;
pub use external_operation_update::ExternalOperationUpdate;

mod orchestration_store;
pub use orchestration_store::OrchestrationStore;
