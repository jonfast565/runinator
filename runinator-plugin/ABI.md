# Runinator Plugin ABI

Runinator plugins use a C-compatible dynamic library boundary with JSON payloads.
The legacy integer-only service ABI has been replaced.

## Required Symbols

```c
int runinator_marker(void);
const char *name(void);
int runinator_abi_version(void);
int call_service(const char *request_json_path, const char *response_json_path);
```

- `runinator_marker` must return `1`.
- `name` returns the provider name used by task `action_name`.
- `runinator_abi_version` must return `1` or higher.
- `call_service` reads a JSON request file and writes a JSON response file.
- A nonzero `call_service` return marks the task failed.

## Cancellation (ABI version 2)

Cancellation crosses the FFI boundary cooperatively through a sentinel file. When the host
cancels an action (worker shutdown, run cancellation, or a lost race branch) it touches a
`cancel.signal` file located **next to** the request's `events_jsonl_path` (the per-run work
directory). An ABI version 2 plugin should:

- Compute the signal path as `dirname(events_jsonl_path)/cancel.signal`.
- Poll for its existence during long-running work and abort cooperatively when it appears.
- Return a nonzero `call_service` value (or a failure message) once it stops early.

ABI version 1 plugins ignore the file and run to completion; the host always writes it, so
newer hosts remain compatible with older plugins. Built-in (in-process) providers receive a
real `CancellationToken` instead and do not use the file.

## Request JSON

```json
{
  "task_id": 42,
  "run_id": 1001,
  "action_name": "Console",
  "action_function": "exec",
  "action_configuration": "echo hello",
  "parameters": { "command": "echo hello" },
  "timeout_secs": 30,
  "artifact_dir": "/tmp/runinator-worker/1001/artifacts",
  "events_jsonl_path": "/tmp/runinator-worker/1001/events.jsonl"
}
```

## Response JSON

```json
{
  "message": "Completed",
  "output_json": { "success": true },
  "chunks": [],
  "artifacts": []
}
```

## Live Events

Plugins may append JSON Lines records to `events_jsonl_path` while running:

```json
{"type":"chunk","stream":"stdout","content":"hello"}
{"type":"artifact","name":"report.csv","mime_type":"text/csv","size_bytes":128,"uri":"/tmp/report.csv","metadata":{}}
{"type":"message","message":"halfway done"}
```

The worker forwards chunk and artifact events to the run API while execution is
still active. Response `chunks` and `artifacts` are persisted after the provider
returns.
