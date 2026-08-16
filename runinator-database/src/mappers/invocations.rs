//! row mappers for resumable invocations and their durable calls.

use super::*;

/// a continuation that fails to decode reads as a fresh one rather than failing the row.
///
/// this is deliberate and narrow: the only way to get here is a continuation written by an ir
/// version this build does not understand, and the vm's own version check is what catches that —
/// with a message naming both versions. failing the mapper instead would make the invocation
/// unreadable, so the run could not even be *inspected* to see why it was stuck.
fn parse_continuation(raw: String) -> InvocationContinuation {
    serde_json::from_str(&raw).unwrap_or_else(|_| InvocationContinuation::start())
}

macro_rules! invocation_from_row {
    ($row:expr) => {{
        WorkflowInvocation {
            id: $row.get("id"),
            workflow_run_id: $row.get("workflow_run_id"),
            workflow_node_run_id: $row.get("workflow_node_run_id"),
            cursor_id: $row.get("cursor_id"),
            node_id: $row.get("node_id"),
            module_version: $row.get::<i64, _>("module_version") as u32,
            continuation: parse_continuation($row.get("continuation")),
            status: WorkflowStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(WorkflowStatus::Failed),
            output: $row.get::<Option<String>, _>("output_json").map(parse_json),
            message: $row.get("message"),
            created_at: $row.get("created_at"),
            updated_at: $row.get("updated_at"),
            finished_at: $row.get("finished_at"),
        }
    }};
}

row_mapper!(row_to_invocation(row) -> WorkflowInvocation { invocation_from_row!(row) });

macro_rules! invocation_call_from_row {
    ($row:expr) => {{
        WorkflowInvocationCall {
            id: $row.get("id"),
            invocation_id: $row.get("invocation_id"),
            workflow_run_id: $row.get("workflow_run_id"),
            sequence: $row.get("sequence"),
            // a target that will not decode is the one field with no safe fallback — there is no
            // "unknown callable" to stand in for it — so it becomes an intrinsic named by the raw
            // text, which fails loudly at dispatch instead of silently calling something else.
            target: serde_json::from_str($row.get::<String, _>("target").as_str()).unwrap_or(
                CallableTarget::Intrinsic {
                    name: $row.get::<String, _>("target"),
                },
            ),
            arguments: serde_json::from_str($row.get::<String, _>("arguments").as_str())
                .unwrap_or_default(),
            policy: serde_json::from_str($row.get::<String, _>("policy").as_str())
                .unwrap_or_default(),
            attempt: $row.get("attempt"),
            status: WorkflowStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(WorkflowStatus::Failed),
            result: $row.get::<Option<String>, _>("result_json").map(parse_json),
            message: $row.get("message"),
            idempotency_key: $row.get("idempotency_key"),
            deadline_at: $row.get("deadline_at"),
            current_executor_replica_id: $row.get("current_executor_replica_id"),
            last_executor_replica_id: $row.get("last_executor_replica_id"),
            executor_claimed_at: $row.get("executor_claimed_at"),
            executor_released_at: $row.get("executor_released_at"),
            created_at: $row.get("created_at"),
            started_at: $row.get("started_at"),
            finished_at: $row.get("finished_at"),
        }
    }};
}

row_mapper!(row_to_invocation_call(row) -> WorkflowInvocationCall { invocation_call_from_row!(row) });
