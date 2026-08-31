//! relational projection for workflow execution state.

use super::*;
use runinator_models::interrupt::{InterruptSource, PendingInterrupt};
use runinator_models::workflow_state::{EventSourceEntry, WorkflowExecutionState};

fn json_value(raw: Option<String>) -> Option<Value> {
    raw.and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .map(Value::from)
}

fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Option<T> {
    serde_json::from_str(raw).ok()
}

pub(super) async fn load<B>(
    store: &B,
    workflow_run_id: Uuid,
) -> Result<WorkflowExecutionState, SendableError>
where
    B: SqlBackend,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    let mut tx = store.pool().begin().await?;
    let base = sqlx::query(&store.render(
        "SELECT watch_fired, run_metadata_json, extra_json FROM workflow_runs WHERE id = ?",
    ))
    .bind(workflow_run_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(base) = base else {
        tx.rollback().await?;
        return Err(Box::new(std::io::Error::other(format!(
            "workflow run {workflow_run_id} has no normalized execution state"
        ))) as SendableError);
    };

    let mut state = WorkflowExecutionState {
        watch_fired: base.get("watch_fired"),
        run_metadata: json_value(base.get("run_metadata_json")),
        extra: serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
            &base.get::<String, _>("extra_json"),
        )
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| (key, Value::from(value)))
        .collect(),
        ..Default::default()
    };

    let frames = sqlx::query(&store.render(
        "SELECT frame_kind, frame_json FROM workflow_run_frames WHERE workflow_run_id = ? ORDER BY frame_kind",
    ))
    .bind(workflow_run_id)
    .fetch_all(&mut *tx)
    .await?;
    for row in frames {
        let kind = row.get::<String, _>("frame_kind");
        let raw = row.get::<String, _>("frame_json");
        match kind.as_str() {
            "control" => state.control = decode(&raw),
            "debug" => state.debug = decode(&raw),
            "map" => state.map = decode(&raw),
            "compensation" => state.compensation = decode(&raw),
            "subflow_parent" => state.subflow_parent = decode(&raw),
            "map_child" => state.map_child = decode(&raw),
            _ => {}
        }
    }

    let cursor_rows = sqlx::query(&store.render(
        "SELECT cursor_id, node_id, forked_by, suspended_by, suspended_seconds, last_output_json FROM workflow_run_cursors WHERE workflow_run_id = ? ORDER BY position",
    ))
    .bind(workflow_run_id)
    .fetch_all(&mut *tx)
    .await?;
    for row in cursor_rows {
        let cursor_id = row.get::<Uuid, _>("cursor_id");
        let mut raw = serde_json::json!({
            "id": cursor_id,
            "node_id": row.get::<String, _>("node_id"),
            "forked_by": row.get::<Option<String>, _>("forked_by"),
            "suspended_by": row.get::<Option<Uuid>, _>("suspended_by"),
            "suspended_seconds": row.get::<i64, _>("suspended_seconds"),
            "last_output": json_value(row.get::<Option<String>, _>("last_output_json"))
        });
        let object = raw.as_object_mut().expect("cursor projection is an object");
        let frame_rows = sqlx::query(&store.render(
            "SELECT frame_kind, position, frame_json FROM workflow_cursor_frames WHERE workflow_run_id = ? AND cursor_id = ? ORDER BY frame_kind, position",
        ))
        .bind(workflow_run_id)
        .bind(cursor_id)
        .fetch_all(&mut *tx)
        .await?;
        let mut loops = Vec::new();
        let mut handled = Vec::new();
        for frame in frame_rows {
            let kind = frame.get::<String, _>("frame_kind");
            let value =
                serde_json::from_str::<serde_json::Value>(&frame.get::<String, _>("frame_json"))
                    .unwrap_or(serde_json::Value::Null);
            match kind.as_str() {
                "loop" => loops.push(value),
                "handled" => handled.push(value),
                "try" | "speculative" | "debug" | "interrupt" => {
                    object.insert(kind, value);
                }
                _ => {}
            }
        }
        if !loops.is_empty() {
            object.insert("loops".into(), loops.into());
        }
        if !handled.is_empty() {
            object.insert("handled".into(), handled.into());
        }
        if let Ok(cursor) = serde_json::from_value(raw) {
            state.cursors.push(cursor);
        }
    }

    let events = sqlx::query(&store.render(
        "SELECT node_id, pending_event_json FROM workflow_run_event_sources WHERE workflow_run_id = ? ORDER BY node_id",
    ))
    .bind(workflow_run_id)
    .fetch_all(&mut *tx)
    .await?;
    for row in events {
        state.event_sources.insert(
            row.get("node_id"),
            EventSourceEntry {
                pending_event: json_value(row.get("pending_event_json")),
            },
        );
    }

    let interrupts = sqlx::query(&store.render(
        "SELECT interrupt_id, source, payload_json, cursor_id, requested_at FROM workflow_run_pending_interrupts WHERE workflow_run_id = ? ORDER BY requested_at, interrupt_id",
    ))
    .bind(workflow_run_id)
    .fetch_all(&mut *tx)
    .await?;
    for row in interrupts {
        let source = row
            .get::<String, _>("source")
            .parse::<InterruptSource>()
            .unwrap_or_default();
        state.pending_interrupts.push(PendingInterrupt {
            id: row.get("interrupt_id"),
            source,
            payload: json_value(row.get("payload_json")).unwrap_or(Value::Null),
            cursor_id: row.get("cursor_id"),
            requested_at: DateTime::<Utc>::from_timestamp(row.get("requested_at"), 0)
                .unwrap_or_else(Utc::now),
        });
    }
    tx.commit().await?;
    Ok(state)
}

pub(super) async fn write<B>(
    store: &B,
    conn: &mut <B::Db as Database>::Connection,
    workflow_run_id: Uuid,
    state: &WorkflowExecutionState,
    clear_existing: bool,
) -> Result<(), SendableError>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    // first writes must not range-delete rows that cannot exist. under mysql's default repeatable
    // read isolation, concurrent deletes of absent child keys take gap locks and two otherwise
    // independent run creations can deadlock when each proceeds to insert its projection.
    if clear_existing {
        for table in [
            "workflow_run_pending_interrupts",
            "workflow_run_event_sources",
            "workflow_run_cursors",
            "workflow_run_frames",
        ] {
            sqlx::query(&store.render(&format!("DELETE FROM {table} WHERE workflow_run_id = ?")))
                .bind(workflow_run_id)
                .execute(&mut *conn)
                .await?;
        }
    }

    let extra = serde_json::to_string(&state.extra).unwrap_or_else(|_| "{}".into());
    sqlx::query(&store.render(
        "UPDATE workflow_runs SET watch_fired = ?, run_metadata_json = ?, extra_json = ? WHERE id = ?",
    ))
    .bind(state.watch_fired)
    .bind(state.run_metadata.as_ref().map(Value::to_string))
    .bind(extra)
    .bind(workflow_run_id)
    .execute(&mut *conn)
    .await?;

    let run_frames: [(&str, Option<String>); 6] = [
        (
            "control",
            state
                .control
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
        ),
        (
            "debug",
            state
                .debug
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
        ),
        (
            "map",
            state
                .map
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
        ),
        (
            "compensation",
            state
                .compensation
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
        ),
        (
            "subflow_parent",
            state
                .subflow_parent
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
        ),
        (
            "map_child",
            state
                .map_child
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
        ),
    ];
    for (kind, raw) in run_frames {
        let Some(raw) = raw else { continue };
        sqlx::query(&store.render(
            "INSERT INTO workflow_run_frames (workflow_run_id, frame_kind, frame_json) VALUES (?, ?, ?)",
        ))
        .bind(workflow_run_id)
        .bind(kind)
        .bind(raw)
        .execute(&mut *conn)
        .await?;
    }

    for (position, cursor) in state.cursors.iter().enumerate() {
        sqlx::query(&store.render(
            "INSERT INTO workflow_run_cursors (workflow_run_id, cursor_id, position, node_id, forked_by, suspended_by, suspended_seconds, last_output_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(workflow_run_id)
        .bind(cursor.id)
        .bind(position as i64)
        .bind(cursor.node_id())
        .bind(cursor.forked_by.clone())
        .bind(cursor.suspended_by)
        .bind(cursor.suspended_seconds)
        .bind(cursor.last_output.as_ref().map(Value::to_string))
        .execute(&mut *conn)
        .await?;

        let mut frames: Vec<(&str, i64, Option<&str>, String)> = Vec::new();
        for (index, frame) in cursor.loops.iter().enumerate() {
            frames.push((
                "loop",
                index as i64,
                Some(frame.node_id.as_str()),
                serde_json::to_string(frame).unwrap_or_else(|_| "{}".into()),
            ));
        }
        if let Some(frame) = &cursor.try_frame {
            frames.push((
                "try",
                0,
                Some(frame.node_id.as_str()),
                serde_json::to_string(frame)?,
            ));
        }
        if let Some(frame) = &cursor.speculative {
            frames.push(("speculative", 0, None, serde_json::to_string(frame)?));
        }
        if let Some(frame) = &cursor.debug {
            frames.push((
                "debug",
                0,
                frame.current_node_id.as_deref(),
                serde_json::to_string(frame)?,
            ));
        }
        if let Some(frame) = &cursor.interrupt {
            frames.push((
                "interrupt",
                0,
                Some(frame.resume.node_id.as_str()),
                serde_json::to_string(frame)?,
            ));
        }
        for (index, key) in cursor.handled.iter().enumerate() {
            frames.push(("handled", index as i64, None, serde_json::to_string(key)?));
        }
        for (kind, index, node_id, raw) in frames {
            sqlx::query(&store.render(
                "INSERT INTO workflow_cursor_frames (workflow_run_id, cursor_id, frame_kind, position, node_id, frame_json) VALUES (?, ?, ?, ?, ?, ?)",
            ))
            .bind(workflow_run_id)
            .bind(cursor.id)
            .bind(kind)
            .bind(index)
            .bind(node_id.map(str::to_string))
            .bind(raw)
            .execute(&mut *conn)
            .await?;
        }
    }

    for (node_id, entry) in &state.event_sources {
        sqlx::query(&store.render(
            "INSERT INTO workflow_run_event_sources (workflow_run_id, node_id, pending_event_json) VALUES (?, ?, ?)",
        ))
        .bind(workflow_run_id)
        .bind(node_id.as_str())
        .bind(entry.pending_event.as_ref().map(Value::to_string))
        .execute(&mut *conn)
        .await?;
    }
    for request in &state.pending_interrupts {
        sqlx::query(&store.render(
            "INSERT INTO workflow_run_pending_interrupts (workflow_run_id, interrupt_id, source, payload_json, cursor_id, requested_at) VALUES (?, ?, ?, ?, ?, ?)",
        ))
        .bind(workflow_run_id)
        .bind(request.id)
        .bind(request.source.as_str())
        .bind((!request.payload.is_null()).then(|| request.payload.to_string()))
        .bind(request.cursor_id)
        .bind(request.requested_at.timestamp())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}
