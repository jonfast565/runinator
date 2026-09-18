//! covers the per-stream checkpoint: independent marks, quiet streams holding their position,
//! the legacy flat form, and seeding a stream an adapter has never marked.

use super::*;
use serde_json::json;

const NOW: &str = "2026-09-17T07:42:52Z";

fn request(checkpoint: Value, initialize: bool) -> AdapterPollRequest {
    AdapterPollRequest {
        configuration: Value::Null,
        secrets: Value::Null,
        checkpoint,
        initialize,
    }
}

#[test]
fn each_stream_keeps_its_own_high_water_mark() {
    // one shared mark let a busy stream drag the watermark past events a quiet one had not
    // emitted yet, silently dropping them. the marks must move independently.
    let mut checkpoints = StreamCheckpoints::open(&request(
        json!({ "streams": { "7:pull_request": "2026-08-27T00:00:00+00:00", "7:workflow_run": "2026-08-27T00:00:00+00:00" } }),
        false,
    ));
    let scope = checkpoints.declare("7", &["pull_request", "workflow_run"], NOW);
    checkpoints.advance(&scope, "pull_request", "2026-08-27T10:00:00+00:00");
    checkpoints.advance(&scope, "workflow_run", "2026-08-27T12:00:00+00:00");
    checkpoints.advance(&scope, "pull_request", "2026-08-27T09:00:00+00:00");

    let checkpoint = checkpoints.into_checkpoint();
    assert_eq!(
        checkpoint
            .pointer("/streams/7:pull_request")
            .and_then(Value::as_str),
        Some("2026-08-27T10:00:00+00:00"),
        "a later mark on another stream must not drag this one forward, and a mark never moves back"
    );
    assert_eq!(
        checkpoint
            .pointer("/streams/7:workflow_run")
            .and_then(Value::as_str),
        Some("2026-08-27T12:00:00+00:00")
    );
    assert_eq!(checkpoint.pointer("/streams/7:check_run"), None);
}

#[test]
fn a_quiet_stream_keeps_its_position_across_a_poll() {
    // rebuilding the map from only the streams that produced events would reset every quiet
    // stream to a cold start, replaying its whole history on the next poll.
    let mut checkpoints = StreamCheckpoints::open(&request(
        json!({ "streams": { "7:check_run": "2026-08-27T08:00:00+00:00", "7:pull_request": "2026-08-27T08:00:00+00:00" } }),
        false,
    ));
    let scope = checkpoints.declare("7", &["pull_request", "check_run"], NOW);
    checkpoints.advance(&scope, "pull_request", "2026-08-27T10:00:00+00:00");

    assert_eq!(
        checkpoints
            .into_checkpoint()
            .pointer("/streams/7:check_run")
            .and_then(Value::as_str),
        Some("2026-08-27T08:00:00+00:00")
    );
}

#[test]
fn a_legacy_flat_checkpoint_stands_in_for_every_stream() {
    // adapters already in flight carry the old single-mark shape; it must keep its position
    // rather than reading as a cold start, and it must not look like an unmarked stream either.
    let mut checkpoints = StreamCheckpoints::open(&request(
        json!({ "updated_at": "2026-08-27T08:00:00+00:00" }),
        false,
    ));
    let scope = checkpoints.declare("7", &["pull_request", "check_run"], NOW);
    assert!(!scope.is_seeded("pull_request"));
    assert_eq!(
        checkpoints.mark(&scope, "pull_request").as_deref(),
        Some("2026-08-27T08:00:00+00:00")
    );
    assert_eq!(
        checkpoints.mark(&scope, "check_run").as_deref(),
        Some("2026-08-27T08:00:00+00:00")
    );
}

#[test]
fn an_unmarked_stream_is_seeded_rather_than_replayed() {
    let kinds = [
        "pull_request",
        "workflow_run",
        "check_run",
        "issue_comment",
        "pull_request_review",
    ];
    // an adapter polling since before reviews and comments were collected keeps its three marks,
    // so exactly the two new streams are seeded and nothing replays.
    let established = json!({ "streams": {
        "1242743236:pull_request": "2026-09-17T07:42:52Z",
        "1242743236:workflow_run": "2026-09-17T07:42:52Z",
        "1242743236:check_run": "2026-09-17T07:42:52Z"
    } });
    let mut checkpoints = StreamCheckpoints::open(&request(established.clone(), false));
    let scope = checkpoints.declare("1242743236", &kinds, NOW);
    let seeded: Vec<&str> = kinds
        .iter()
        .copied()
        .filter(|kind| scope.is_seeded(kind))
        .collect();
    assert_eq!(seeded, vec!["issue_comment", "pull_request_review"]);
    // a seeded stream reports no mark, so its poller has nothing to enumerate this pass.
    assert_eq!(checkpoints.mark(&scope, "issue_comment"), None);
    assert_eq!(
        checkpoints.mark(&scope, "pull_request").as_deref(),
        Some("2026-09-17T07:42:52Z")
    );
    // and the seed is written, so the stream continues from this poll rather than seeding again.
    assert_eq!(
        checkpoints
            .into_checkpoint()
            .pointer("/streams/1242743236:issue_comment")
            .and_then(Value::as_str),
        Some(NOW)
    );

    // a different subject in the same adapter shares no marks with this one.
    let mut checkpoints = StreamCheckpoints::open(&request(established, false));
    let other = checkpoints.declare("999", &kinds, NOW);
    assert!(kinds.iter().all(|kind| other.is_seeded(kind)));
}

#[test]
fn an_initializing_poll_seeds_every_declared_stream() {
    // a first poll establishes the boundary without scanning history, even where a mark survives
    // from an earlier revision of the same adapter.
    let mut checkpoints = StreamCheckpoints::open(&request(
        json!({ "streams": { "7:pull_request": "2026-08-27T08:00:00+00:00" } }),
        true,
    ));
    let scope = checkpoints.declare("7", &["pull_request", "check_run"], NOW);
    assert!(scope.is_seeded("pull_request"));
    assert!(scope.is_seeded("check_run"));
    assert_eq!(checkpoints.mark(&scope, "pull_request"), None);
}
