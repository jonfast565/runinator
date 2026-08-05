//! the append-only trails: dead letters and the audit log.

use super::*;

#[tokio::test]
async fn dead_letters_and_audit_log_round_trip() {
    let path = std::env::temp_dir().join(format!(
        "runinator-dlq-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let event_id = Uuid::now_v7();
    let stored = db
        .record_dead_letter(runinator_models::json!({
            "channel": "result",
            "event_id": event_id.to_string(),
            "dedupe_key": "poison-1",
            "attempts": 3,
            "error": "forced failure",
            "payload": {"kind": "chunk"},
        }))
        .await
        .unwrap();
    assert_eq!(
        stored.get("channel").and_then(Value::as_str),
        Some("result")
    );
    assert_eq!(stored.get("attempts").and_then(Value::as_i64), Some(3));

    // filter by channel, and confirm a non-matching channel returns nothing.
    let all = db.fetch_dead_letters(None, 50).await.unwrap();
    assert_eq!(all.len(), 1);
    let filtered = db
        .fetch_dead_letters(Some("ingress".into()), 50)
        .await
        .unwrap();
    assert!(filtered.is_empty());
    assert_eq!(
        all[0]
            .get("payload")
            .and_then(|p| p.get("kind"))
            .and_then(Value::as_str),
        Some("chunk")
    );

    let actor = Uuid::now_v7();
    db.record_audit_log(runinator_models::json!({
        "actor_id": actor.to_string(),
        "actor_kind": "user",
        "action": "auth.login",
        "outcome": "success",
        "detail": "ok",
    }))
    .await
    .unwrap();
    db.record_audit_log(runinator_models::json!({
        "actor_kind": "anonymous",
        "action": "auth.login",
        "outcome": "failure",
        "detail": "bad password",
    }))
    .await
    .unwrap();

    let by_actor = db.fetch_audit_log(Some(actor), None, 50).await.unwrap();
    assert_eq!(by_actor.len(), 1);
    assert_eq!(
        by_actor[0].get("outcome").and_then(Value::as_str),
        Some("success")
    );
    let by_action = db
        .fetch_audit_log(None, Some("auth.login".into()), 50)
        .await
        .unwrap();
    assert_eq!(by_action.len(), 2);

    let _ = std::fs::remove_file(path);
}
