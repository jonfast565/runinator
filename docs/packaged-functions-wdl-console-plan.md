# Packaged Functions and WDL Console Implementation Plan

## Objective

Add immutable packaged code that can be invoked through an API, called naturally from WDL, and
executed interactively through a durable WDL Console in the command center.

The implementation should use one execution architecture:

- packaged functions compile to typed action calls;
- direct function invocation creates a generated workflow run;
- effectful console submissions create durable scratch workflow runs; and
- the existing reducer, action outbox, broker, worker result, cancellation, artifact, and tracing
  paths remain authoritative.

This avoids creating a second function-specific orchestration engine.

## Architectural Decisions

1. A packaged function remains an `action` node; do not add a function node kind.
2. One package may export multiple typed functions.
3. WDL calls use a qualified surface such as `functions.image_tools.resize(...)`.
4. Workflow revisions pin exact function versions during compilation or import.
5. Direct API invocation runs a hidden generated adapter workflow.
6. Effectful console submissions run hidden, retention-managed scratch workflows.
7. Pipelines are console-level commands, not workflow nodes.
8. Published artifacts are immutable and content-addressed.
9. Function execution uses a runtime abstraction, initially backed by Docker.
10. Function catalog entries come from durable deployment records, not live worker registrations.

## Phase 1: Function Domain and Persistence

Add shared models in `runinator-models`:

```rust
FunctionPackage
FunctionVersion
FunctionExport
FunctionAlias
FunctionArtifact
FunctionRuntimeSpec
FunctionResourceLimits
FunctionBinding
FunctionInvocationContext
```

Add the following tables:

```text
function_packages
function_versions
function_exports
function_aliases
function_artifacts
function_adapter_workflows
```

A package owns versions, a version owns exports, and an alias points to a package version so every
export in that package advances atomically.

Add a `FunctionStore` role to `runinator-store`, with its generic SQL implementation in
`runinator-database/src/operations/functions.rs`. Keep this separate from `DefinitionStore`.

Required invariants:

- package names are unique within an organization and namespace;
- published versions are immutable;
- artifact digests are immutable;
- aliases may move, but existing workflow bindings remain pinned;
- artifact deletion is blocked while any function version, workflow revision, or console cell
  references it; and
- function resources participate in `View`, `Run`, `Edit`, and `Own` grants.

### Acceptance Gate

Package, version, export, artifact, and alias operations work across SQLite, Postgres, and MySQL.

## Phase 2: Artifact and Manifest Format

Start with source archives rather than user-supplied OCI images.

Example package:

```text
runinator-function.toml
src/
  images.py
schemas/
  resize-input.json
  resize-output.json
```

Example manifest:

```toml
[package]
name = "image-tools"
runtime = "python3.13"

[[exports]]
name = "resize"
handler = "src.images.resize"
input = "schemas/resize-input.json"
output = "schemas/resize-output.json"
timeout_seconds = 30
memory_mb = 512
```

Define an artifact storage interface:

```rust
trait FunctionArtifactStore {
    async fn put(&self, digest: &str, bytes: Bytes) -> Result<ArtifactLocation>;
    async fn open(&self, digest: &str) -> Result<ArtifactReader>;
    async fn delete(&self, digest: &str) -> Result<()>;
}
```

Implement local filesystem storage first while leaving room for S3 or OCI-backed implementations.

Add CLI commands:

```text
runinatorctl functions validate ./image-tools
runinatorctl functions publish ./image-tools
runinatorctl functions versions image-tools
runinatorctl functions alias image-tools production 3
```

Publishing should:

1. validate the manifest and schemas;
2. canonicalize and archive the package;
3. compute its SHA-256 digest;
4. upload the artifact if it is not already present;
5. create an immutable version;
6. materialize its exports; and
7. optionally move an alias.

### Acceptance Gate

The same package produces the same digest, and republishing identical bytes does not duplicate
artifact storage.

## Phase 3: Function Execution Runtime

Create a focused `runinator-function-runtime` crate with an interface such as:

```rust
trait InvocationRuntime {
    fn execute(
        &self,
        spec: &ResolvedFunctionInvocation,
        sink: Arc<dyn ProviderEventSink>,
        cancellation: CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError>;
}
```

Implement `DockerInvocationRuntime` first. Extract reusable mechanics from the existing Docker-backed
foreign-code executor rather than building a second container runner.

Add `runinator-provider-functions`, responsible only for executing a prepared package through an
`InvocationRuntime`.

The worker should:

1. receive the pinned function version and export identity;
2. resolve and download the artifact through `runinator-api`;
3. verify its digest;
4. place it in a bounded local cache;
5. pass the local artifact and invocation specification to the provider;
6. stream logs and artifacts through the existing provider event sink; and
7. publish the ordinary terminal result.

The provider must not fetch control-plane metadata itself.

Initial sandbox requirements:

- memory, CPU, PID, and timeout limits;
- read-only package mount;
- bounded writable temporary storage;
- non-root container user;
- dropped Linux capabilities;
- network disabled by default;
- bounded stdout and stderr; and
- forced container termination on timeout or cancellation.

### Acceptance Gate

A packaged Python function can be executed by a worker, canceled, timed out, and have its result and
logs persisted through the existing result-event path.

## Phase 4: Catalog and WDL Integration

Synthesize provider metadata from published exports:

```text
functions.image_tools.resize
functions.image_tools.inspect
```

Each export becomes typed action metadata whose parameters and results come from its schemas. The
catalog must remain available when the worker pool is scaled to zero.

Extend compilation so a synthetic function call lowers to an ordinary action with a hidden pinned
binding:

```rust
struct FunctionBinding {
    package_id: Uuid,
    version_id: Uuid,
    export_id: Uuid,
    artifact_digest: String,
}
```

For example:

```wdl
node image <- functions.image_tools.resize(
    source: input.source,
    width: 320
)
```

conceptually lowers to:

```json
{
  "kind": "action",
  "action": {
    "provider": "functions",
    "function": "invoke",
    "configuration": {
      "input": {
        "source": {"$ref": "input.source"},
        "width": 320
      }
    },
    "function_binding": {
      "version_id": "...",
      "export_id": "...",
      "artifact_digest": "sha256:..."
    }
  }
}
```

The backend must validate the binding during import so hand-authored JSON cannot reference another
tenant's function.

Place cross-stage format, type, compile, lower, and decompile tests in `runinator-wdl`. Cover:

- completion and hover;
- parameter checking;
- output inference;
- formatting and decompilation;
- alias resolution;
- missing or deleted aliases;
- exact version pinning; and
- old workflow revisions after an alias moves.

### Acceptance Gate

A saved workflow invokes a packaged function with the same retry, timeout, transition,
compensation, routing, and idempotency behavior as an ordinary provider action.

## Phase 5: Direct Invocation Through Adapter Workflows

When a function version is published, generate one hidden adapter workflow per export:

```text
start -> functions.invoke -> output -> end
                         \-> failure -> fail
```

Store the relationship in `function_adapter_workflows`.

Add APIs:

```text
POST /functions/{package}/{export}/invocations
POST /functions/{package}/{export}/invocations?alias=production
GET  /function_invocations/{run_id}
POST /function_invocations/{run_id}/cancel
GET  /function_invocations/{run_id}/logs
GET  /function_invocations/{run_id}/artifacts
```

The invocation endpoint should:

1. resolve the alias to an exact version;
2. authorize `Permission::Run`;
3. validate the input;
4. start the adapter workflow; and
5. return its workflow-run ID as the invocation ID.

Support:

- `Prefer: respond-async` returning `202 Accepted`;
- a bounded synchronous wait returning `200` when the invocation settles quickly;
- fallback to `202` when the synchronous wait expires; and
- `Idempotency-Key` scoped to organization and function export.

Do not create a separate invocation engine or duplicate node-run records.

### Acceptance Gate

Invoking through HTTP and invoking from WDL produce equivalent outputs, logs, artifacts, timeout
behavior, cancellation behavior, and tracing.

## Phase 6: Function Management UI

Add a Functions area to the command center with:

- package list;
- export signatures;
- version history;
- alias management;
- publish and upload actions;
- input and output schema viewers;
- referencing workflows;
- invocation history;
- test invocation form;
- logs and artifacts; and
- an **Open in WDL Console** action.

Keep services under `core/services`, wire models under `core/domain/models`, and Vue presentation
under `ui/`.

### Acceptance Gate

A user can publish, test, promote, inspect, and invoke a function without using the CLI.

## Phase 7: Console Execution Backend

Add models:

```rust
ConsoleSession
ConsoleCell
ConsoleCellKind
ConsoleCellStatus
ConsoleBinding
```

Add tables:

```text
console_sessions
console_cells
console_bindings
```

Add a `ConsoleStore` role instead of attaching console operations directly to `DatabaseImpl`.

Create a small `runinator-console` library that classifies and normalizes submissions:

```rust
enum ConsoleSubmission {
    PureExpression,
    ActionFragment,
    WorkflowInvocation,
    PipelineInvocation,
    WorkflowDocument,
}
```

Route submissions as follows:

| Submission | Behavior |
| --- | --- |
| Pure expression | Use the existing pure fragment evaluator without creating a run. |
| Action or function fragment | Generate a hidden scratch workflow. |
| Single `subflow(...)` | Start the referenced workflow directly. |
| Pipeline command | Start the existing pipeline run directly. |
| Full WDL document | Persist a hidden revision and run its selected entrypoint. |

Add APIs:

```text
POST /console/sessions
GET  /console/sessions/{id}
POST /console/sessions/{id}/cells
GET  /console/cells/{id}
POST /console/cells/{id}/cancel
POST /console/cells/{id}/replay
POST /console/cells/{id}/promote
```

Persist effectful cells before executing them. Scratch definitions should carry metadata such as:

```json
{
  "execution_origin": "console",
  "console_session_id": "...",
  "console_cell_id": "...",
  "ephemeral": true
}
```

Reject schedules and durable triggers in scratch documents.

### Acceptance Gate

A user can submit a function call, disconnect the browser, reconnect, and recover the live run with
its source, status, logs, and results intact.

## Phase 8: WDL Console UI

Implement a notebook-style console with:

- CodeMirror WDL editing;
- completion, hover, formatting, and diagnostics;
- input JSON editor;
- separate **Analyze** and **Run** actions;
- result, logs, artifacts, timeline, and generated-WDL views;
- running-cell cancellation;
- replay;
- open-in-debugger;
- save or promote as workflow;
- command history; and
- concurrent running cells.

Start without mutable cross-cell variables. Add explicit named result bindings afterward:

```wdl
session.thumbnail.uri
```

A binding stores a snapshot of a previous JSON result or a durable artifact reference. It must not
dynamically reread mutable prior state.

Console-only meta-commands may include:

```text
:help
:history
:cancel <run-id>
:open <run-id>
:bindings
:save workflow <name>
:run pipeline <name> <json>
```

### Acceptance Gate

All submission categories work in the UI, with authorization independently enforced by the backend.

## Phase 9: Pack Integration

Extend the compiled-pack wire contract:

```text
workflows.json
functions.json
pipelines.json
secrets.json
function-artifacts/<sha256>.zip
```

Update every writer and reader together: ctl, API, command center, MCP, utilities, and web service.

Import order:

1. store and verify artifacts;
2. reconcile packages and versions;
3. materialize function catalog entries;
4. compile or validate workflow bindings;
5. reconcile pipelines and settings; and
6. commit activation atomically.

A removed package version remains retained while any workflow revision or console cell references
it.

### Acceptance Gate

A single pack can atomically deploy a package and workflows that call its exports, and a failed
function or workflow validation activates none of the incoming resources.

## Phase 10: Policy, Metering, and Retention

Before broad deployment, add:

- function deployment and invocation capabilities;
- function resource grants;
- per-function reserved concurrency;
- per-organization invocation quotas;
- runtime duration and memory metering;
- artifact and log byte accounting;
- console effect analysis;
- audit events for publish, alias movement, invocation, and console execution;
- console and scratch-definition retention;
- artifact garbage collection;
- worker cache eviction; and
- dead-letter enrichment with function and console identity.

## Recommended Pull Request Sequence

1. Function models, migrations, and `FunctionStore`.
2. Artifact store and manifest validation.
3. Function publish, list, version, and alias APIs plus CLI commands.
4. Runtime trait and Docker executor.
5. Worker artifact cache and packaged execution.
6. Durable synthetic catalog metadata.
7. WDL lowering, typing, IDE support, and round-trip coverage.
8. Generated adapter workflows.
9. Direct invocation API.
10. Function management UI.
11. Console session and cell persistence.
12. Console classifier and scratch execution.
13. WDL Console UI.
14. Pack integration.
15. Quotas, metering, retention, and security hardening.

## End-to-End Release Test

The first complete release should prove this scenario:

1. Publish `image-tools` with `resize` and `inspect` exports.
2. Assign version 3 to the `production` alias.
3. Invoke `resize` through HTTP.
4. Invoke it from a saved WDL workflow.
5. Invoke it from a console action fragment.
6. Run a free-form console workflow containing both exports.
7. Launch an existing workflow and pipeline from the console.
8. Move `production` to version 4.
9. Confirm new API calls use version 4.
10. Confirm saved workflow revisions and console replays remain pinned to version 3.
11. Confirm cancellation, retry, logs, artifacts, authorization, and tracing behave identically
    across every entry path.

At that point, packaged functions and the WDL Console are native Runinator concepts rather than
adjacent execution features.
