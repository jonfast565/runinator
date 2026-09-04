# Durable workspaces

A durable workspace holds files and named JSON results under an organization-scoped key. Each
successful writing step creates an immutable version in the shared object store. A different
worker can restore that version without the original machine. Existing worker-local workspace
leases continue to use their existing affinity format.

## Authoring

A workflow-wide binding attaches all otherwise-unbound actions and adds a completion checkpoint:

```rexrap
namespace acme.reports
workflow "Report" v1 {
    key report
    workspace { key: "monthly-report", create: true }
    do {
        let generated = console.run(command: "echo report > report.txt")
    }
}
```

An action can override the default with `@workspace({ key: "monthly-report", version: 3 })`.
Bindings accept `access: "read"` for an immutable reader; write is the default. `create: true`
creates a missing key with the consuming workflow's ownership. Otherwise missing keys fail.
Keys contain 1–200 bytes, without surrounding whitespace or control characters.

The provider receives the restored workspace directory through its ordinary `workspace_path`
contract. Providers that support workspace paths can read or produce files there. The directory
exists for that execution only; store durable references, never the local absolute path.

Each version saves the provider's output under the named result `result`. Additional mappings
can be declared on the attachment:

```json
{
  "key": "monthly-report",
  "version": 3,
  "results": { "summary": { "$output": "/summary" } }
}
```

The next attached action can read saved data in its configuration with
`{ "$workspace": "/summary" }`. The JSON pointer is resolved against the restored version's
named results before provider input validation. Missing pointers fail the step. Existing named
results remain until replaced; `result` is replaced after each writing step.

Successful attached actions expose `output.workspace = { "key": "…", "version": 4 }`.
For scalar provider output, the original value is returned under `output.result`. Pass that
workspace reference as the next workflow's input and bind its workspace to `params.workspace`.
Always pass the version when handing off a particular result; a bare key initially selects head.

Pipeline defaults and individual member overrides support the same binding:

```rexrap
pipeline "Report processing" {
    workspace { key: "monthly-report", create: true }
    workflow "acme.reports.generate"
    workflow "acme.reports.publish" with_workspace params.workspace
    "acme.reports.generate" -> "acme.reports.publish" on success
        with { workspace: source.outputs.workspaces["monthly-report"] }
}
```

Pipeline member results include `outputs.workspaces`, a map of keys to their latest committed
references. Workflow defaults follow their own run's committed version; at the beginning of a
pipeline member they can continue the same pipeline run's saved version. Explicit action
attachments remain pinned to the supplied version. A workflow completion checkpoint saves its
final result using the ordinary provider-effect settlement path.

## Versions, retries, and storage

Only one writing checkout is active per workspace. Other writers wait; readers can restore any
retained immutable version concurrently. A writer whose pinned base no longer matches head fails
with a conflict. It never silently overwrites another run's work. Use distinct keys for independent
parallel branches or runs that should not share mutable state.

Snapshot metadata, the new head, and effect settlement commit in one database transaction.
Checkout fences reject expired or superseded attempts. Failure, timeout, or cancellation does not
publish a version. Retries restore their original committed base. Use an effect-specific
idempotency key for writes; cached mutations cannot be replayed under another effect's identity.

Snapshot bytes are gzip-compressed tar archives in the `runinator-workspaces` object-store bucket.
They include `files/` and `results.json`; checksums, file sizes, executable flags, and relative link
metadata are retained. Limits are 512 MiB compressed, 2 GiB expanded, 100,000 archive entries, and
16 MiB of named results. Unsafe paths, special files, duplicate archive paths, and escaping or
cyclic symlinks are rejected. Symlink restoration currently requires Unix.

Expanded worker copies live under the platform application-data `portable-workspaces` directory.
They are removed after execution. A minute-based sweeper removes expired copies left by crashes.
The checkout lease covers the action timeout plus five minutes for transfer and settlement.
Unused uploaded archives are collected after 24 hours once their effect is terminal. Committed
versions are retained until explicitly deleted; this feature introduces no automatic version
retention limit.

## Inspecting and deleting

Command Center's **Workspaces** view lists keys, version history, producing run/attempt, saved
results, and the file manifest. Download a compressed version or individual regular files. Pipeline
defaults also include a JSON workspace-binding editor; member overrides can be authored in REXRAP.

Backend permissions use the existing resource ownership registry: view permits inspection and
download; edit permits historical-version deletion; own permits deleting the entire workspace.
Workers can upload or restore only a live checkout assigned to their registered replica.

An active checkout or active workflow/pipeline reference protects the relevant version from
deletion. The current head cannot be deleted separately. Whole-workspace deletion tombstones the
key and all versions, then removes bytes with retryable background cleanup. Deleted keys remain
reserved so stale key/version references cannot accidentally point at a replacement workspace.

HTTP management routes are `/workspaces`, `/workspaces/{id}`, and
`/workspaces/{id}/versions`. Downloads use `/workspaces/{id}/versions/{version}/content`, optionally
with `?path=relative/file`. Worker transfer uses `/workspaces/checkouts/{checkout}/content` and the
assigned `replica_id`. Shared payloads live in `runinator-models`; the store contract and SQL live
in `runinator-store` and `runinator-database`. Engine repository services own orchestration and
blob operations. `runinator-workspace` owns archive and result-reference handling.
