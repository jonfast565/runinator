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

## Passing a workspace name through a pipeline

A workspace name is its key. Pass it as a string pipeline parameter, for example
`{ "workspace_name": "monthly-report-2026-09" }`. Each member receives the pipeline parameters;
links with no parameter mapping preserve them. Every consuming workflow must declare the
parameter in its `params` block, before `key` and other header declarations:

```rexrap
namespace acme.reports
workflow "Generate report" v1 {
    params { workspace_name: string }
    key generate
    workspace { key: params.workspace_name, create: true }
    do {
        let generated = console.run(command: "echo report > report.txt")
    }
}

workflow "Read report" v1 {
    params { workspace_name: string }
    key read
    workspace { key: params.workspace_name, access: "read" }
    do {
        let report = console.run(command: "cat report.txt")
    }
}

pipeline "Named report workspace" {
    workflow "acme.reports.generate"
    workflow "acme.reports.read"
    "acme.reports.generate" -> "acme.reports.read" on success
}
```

Start this pipeline with `workspace_name` in its run parameters. The first workflow creates a
missing workspace and saves its files; the second restores the same pipeline run's saved version.
The name is supplied at run time, so these workflows can be reused with different workspace names.

A pipeline can also supply the binding for a workflow that has no workspace header:

```rexrap
workflow "acme.reports.transform" with_workspace { key: params.workspace_name }
```

Or set `workspace { key: params.workspace_name, create: true }` as the pipeline default to bind
all members. A member override takes precedence over that default, and both take precedence over
the workflow's own workspace header. Keep `access: "read"` on a member override when that member
should only inspect the shared workspace.

To explicitly forward or rename the parameter on an edge, use a parameter mapping:

```rexrap
"acme.reports.generate" -> "acme.reports.read" on success
    with { workspace_name: params.workspace_name }
```

The string belongs in the binding's `key` field; `workspace params.workspace_name` expects a
complete binding object and is not the string form. Sequential members can share a name; use
different names for independent parallel writers. Pass a `{ key, version }` reference instead
when the consumer must use a particular immutable version.

## Versions, retries, and storage

Only one writing checkout is active per workspace. Other writers wait; readers can restore any
retained immutable version concurrently. A writer whose pinned base no longer matches head fails
with a conflict. It never silently overwrites another run's work. Use distinct keys for independent
parallel branches or runs that should not share mutable state.

Snapshot metadata, the new head, and effect settlement commit in one database transaction.
Checkout fences reject expired or superseded attempts. Failure, timeout, or cancellation does not
publish a version. Retries restore their original committed base. Use an effect-specific
idempotency key for writes; cached mutations cannot be replayed under another effect's identity.

`runinator-workspace-storage` owns the immutable storage algorithms. `runinator-workspace`
adds canonical named results, OCI transfers, and provider-directory reconciliation. Workers scan
file contents even when timestamps match; unchanged inode identities and hard-link groups are
preserved. Files use size-selected logical pages, FastCDC chunks, holes, tiny-object blocks, and
bounded dictionary/delta compression groups. Repacking preserves typed BLAKE3 logical IDs.
Namespace and path-projection roots change together. New objects live in bounded packs in the
`runinator-workspaces` blob bucket; SQL maps workspace-scoped logical IDs to validated pack ranges.
No replica-local repository ref or index is authoritative.

Uploading objects does not publish a version. The server validates the selected closure, namespace,
results, links, parent and frozen limits, then issues a checkout/fence/effect/attempt-bound receipt.
Successful effect settlement consumes that exact receipt and publishes the head atomically. A
forged receipt, changed snapshot, expired checkout or losing attempt cannot publish.

Both `runinator-ws` and `runinator-engine-worker` accept positive integer settings:

| Environment variable | CLI option | Default |
| --- | --- | --- |
| `RUNINATOR_WORKSPACE_MAX_BYTES` | `--workspace-max-bytes` | 137438953472 (128 GiB) |
| `RUNINATOR_WORKSPACE_MAX_ENTRIES` | `--workspace-max-entries` | 100000 |
| `RUNINATOR_WORKSPACE_MAX_RESULTS_BYTES` | `--workspace-max-results-bytes` | 16777216 (16 MiB) |

Policy is frozen into a checkout or import job. Total logical bytes count each distinct regular-file
inode once, including holes, plus the canonical named-results map. Entries count every namespace
name except the root, including directories and aliases. Named results are individually paged and
may exceed a single object when the result policy permits. Provider execution resolves results into
memory; increasing that policy also increases provider memory requirements. Configure both hosts
consistently. The supervisor example and Kubernetes common ConfigMap carry these defaults.
Provision enough object-store and worker scratch capacity for retained data and temporary copies;
logical policy does not resize a persistent volume.

Expanded worker copies live under the platform application-data `portable-workspaces` directory
and are removed after execution. Expired crash leftovers are swept. The action deadline covers
restore, execution, content scanning, upload and sealing. Unsafe paths, special files, and escaping
or cyclic relative symlinks are rejected. Symlink restoration currently requires Unix.

Restore is one native archive request per checkout. The engine resolves the revision through
pack-batched location pages and bounded local pack copies, and the worker imports that archive into
a local logical store retained for the full action before materialization. A logical object lookup
must not cross the HTTP boundary; the object endpoint is a compatibility fallback, not the normal
restore path.

Authenticated checkout seal requests retain the HTTP concurrency cap but use the checkout lease
deadline instead of the generic 30-second request timeout. Dropping or expiring validation stops
further storage reads and prevents receipt issuance. The client's seal request uses the worker's
remaining action budget rather than its generic 60-second API timeout.

Seal validation loads registered object locations in pages of 1,000 and reads from disposable local
pack copies, avoiding a SQL lookup and remote blob request for every object. It retains at most
eight packs of at most 80 MiB each, disk-backed location indexes, a 48 MiB logical-object cache, and
a 64 MiB decoded-record cache. Shared compression blocks are verified and decoded once while
resident, rather than once per logical member.
Only registered locations are indexed; reading each reachable object still checks its identity,
and the complete graph, usage, results, and portable links are validated before receipt issuance.

Run the controlled local comparison with
`cargo test -p runinator-engine compare_seal_validation_backends -- --ignored --nocapture`.
It validates the same registered packs through both readers and reports elapsed time and the new
reader's database/blob call counts. The fixture contains 1,001 files and about 36 MiB of data.

Retained versions, pending valid receipts and active transfers are explicit collection roots;
parent revision metadata does not retain ancestor contents. Collection uses fenced SQL leases,
rebuilds the logical index atomically and defers old-pack deletion while readers hold renewable
leases. Active imports and writers exclude collection. Unreferenced uploads are swept after 24
hours. Versions are retained until explicitly deleted; there is no automatic history limit.

## Archive transfers

Native OCI layout tar exports preserve the selected revision's filesystem and named results.
Conventional OCI exports contain a filesystem checkpoint only. Conventional imports apply whiteouts
and layers in order and start with empty named results. Imports accept exactly one manifest;
select one manifest before uploading a multi-platform layout. Imports require an unused workspace
key and publish version 1 with explicit import provenance, never fabricated workflow identifiers.

Transfers are durable jobs with progress, cancellation, lease fencing and seven-day expiry. A
replica can reclaim an interrupted job. Uploads and downloads stream; archive bytes live in shared
blob storage. Buffers and packs are bounded. Expanded OCI layers and archive staging have a checked
budget derived from the frozen policy. Native exports use bounded scratch packs; conventional
exports stage a filesystem tar checkpoint. Transfer uploads use a 60-second idle-read deadline;
ordinary API requests keep their existing request timeout.

```sh
runinatorctl workspaces list
runinatorctl workspaces versions WORKSPACE_UUID
runinatorctl workspaces ls WORKSPACE_UUID 1 --results
runinatorctl workspaces cat WORKSPACE_UUID 1 report.txt
runinatorctl workspaces diff WORKSPACE_UUID 1 2
runinatorctl workspaces import new-key workspace.oci.tar
runinatorctl workspaces export WORKSPACE_UUID 1
runinatorctl workspaces export WORKSPACE_UUID 1 --filesystem
runinatorctl workspaces job TRANSFER_UUID
runinatorctl workspaces download TRANSFER_UUID workspace.oci.tar
runinatorctl workspaces cancel TRANSFER_UUID
```

`cat` is a bounded preview (at most 1 MiB). Directory/results/diff responses include continuation
cursors bound to immutable roots. Use `--cursor` to continue. Native CLI commands, console parsing,
MCP schemas and WASM catalog data derive from the same command tree.

## Inspecting and deleting

Command Center's **Workspaces** view lists keys, version history, producing run/attempt, paged
results, directories and diffs. Preview files or download individual files/results; archive downloads
create durable export jobs. Browser downloads use short-lived resource-scoped tickets and desktop
downloads stream directly to a selected file. Pipeline
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
assigned `replica_id`; one response contains the complete native revision archive. Shared payloads
live in `runinator-models`; the store contract and SQL live in `runinator-store` and
`runinator-database`. Engine repository services own orchestration and blob operations.
`runinator-workspace` owns archive and result-reference handling.

## New-format cutover

The previous gzip archive format has no compatibility path. This release requires an empty durable
workspace registry before admission resumes. The repository's PostgreSQL/FsBlob Kubernetes stack
has a restartable cutover command under the ordinary deployment lease:

```sh
cargo run -p xtask -- k8s reset-workspaces --discard-workspaces
cargo run -p xtask -- k8s deploy
cargo run -p xtask -- k8s reset-workspaces --resume
```

First cancel workspace-dependent runs through the normal run/pipeline API; the reset preflight
reports and refuses active dependencies. It records replica counts and phases in the
`runinator-workspace-storage-cutover` ConfigMap, quiesces service/worker deployments, rechecks
active dependencies, erases only durable workspace tables and workspace grants/ownership, then
clears only the durable-workspace blob bucket. Workflow definitions, runs, artifacts, credentials,
other buckets and worker user directories are untouched. A repeated interrupted reset resumes
safely; a completed ledger refuses another erase so newly created workspaces cannot be deleted by
an accidental retry. Resume recorded replicas only after deploying the new storage code. External
object stores require an equivalent scoped maintenance procedure; the command refuses them.
