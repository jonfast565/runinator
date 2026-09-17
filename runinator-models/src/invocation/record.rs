//! the persisted shape of a running invocation and the durable calls it makes.
//!
//! these are the rows, not the ir: [`InvocationContinuation`] is the frozen program state and lives
//! inside [`WorkflowInvocation::continuation`], while a [`WorkflowInvocationCall`] is one durable
//! call the vm yielded on. the split matters because they have different lifetimes — a continuation
//! is rewritten on every yield and resume, and a call row is written once and settled once.
//!
//! status uses [`WorkflowStatus`] rather than a private vocabulary. an invocation and its calls sit
//! in the same lifecycle every other unit of work does, and a second set of names would have to be
//! mapped at every boundary that already knows this one.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{CallPolicy, CallableTarget, InvocationContinuation};
use crate::value::Value;
use crate::workflows::WorkflowStatus;

mod workflow_invocation;
pub use workflow_invocation::WorkflowInvocation;

mod workflow_invocation_call;
pub use workflow_invocation_call::WorkflowInvocationCall;

mod new_invocation_call;
pub use new_invocation_call::NewInvocationCall;
