# REXRAP CLI and pack workflow

RexRap source is compiled on the client. The web service imports compiled pack artifacts; it does
not parse `.rrx` files or run a backend compiler service.

## Check, format, compile, and decompile

```bash
runinatorctl rexrap check workflow.rrx
runinatorctl rexrap format workflow.rrx --check
runinatorctl rexrap compile workflow.rrx -o workflow.json
runinatorctl rexrap decompile workflow.json -o workflow.rrx
runinatorctl rexrap decompile workflow.json --explicit
```

`check` reports parser and semantic diagnostics with source spans. RexRap commands default to
strict typing. `--typing permissive` exists for investigating legacy definitions; pack imports stay
strict.

`decompile --explicit` emits inferred entry edges, sequential transitions, generated defaults, and
other normally implicit graph structure. Compile → decompile → compile and formatter idempotence
are cross-stage contracts tested in this facade crate.

## Import a pack

`runinatorctl workflows apply <path>` accepts one unified `.rrx` file, a directory of `.rrx`
sources, or one standalone workflow-definition JSON file. It does not accept an already assembled
workflow bundle.

For RexRap input, the CLI:

1. reads every workflow signature in the source set;
2. compiles each workflow with the complete pack-local signature table;
3. writes `workflows.json` and optional versioned `settings.json` and `pipelines.json` entries into one pack
   zip; and
4. uploads that zip as `application/zip` to `/packs/import`.

The two-pass compile makes subflow parameters and outputs type-check even when the target workflow
appears later or in another file. The pack entry layout is a shared wire contract implemented by
`runinator-pack-wire`.

Reapplying a pack replaces its stored definitions and reconciles pack-managed triggers,
notifications, and pipeline links. Accepted definitions are also captured as immutable revisions;
rolling back creates another revision instead of rewriting history.

## Development loop and tests

`runinatorctl workflows dev <path>` watches the source set, recompiles it client-side, and reapplies
the pack after a successful change. With `--run`, it starts the selected workflow and follows the
run to a terminal state.

`runinatorctl workflows test <path>` runs `.rrx` `tests { ... }` blocks locally with mocked task
outputs. It uses the graph simulator rather than the durable VM, so it is appropriate for routing
and output assertions but not for effect delivery or recovery tests. See [Runtime model](runtime.md)
for the distinction.

## Unified source blocks

A `.rrx` file can contain workflow, pipeline, settings, package-manifest, and test blocks. The pack
front end separates those blocks and compiles them into their corresponding artifacts.

Settings declarations use dotted addresses and JSON-literal values:

```rexrap
secret jira.token    = "abc123"
config jira.base_url = "https://acme.atlassian.net"
config app.flags     = { beta: true, region: "us" }
```

Use `runinatorctl settings import settings.rrx` for a standalone settings import. Secret values are
stored as secrets and resolved late by workers; config values are available in the run's resolved
configuration snapshot.

Pipeline blocks compile alongside workflows. Import resolves workflow names to IDs, stores the
immutable pipeline revision, and materializes its managed links and ingress/orchestration policy.

See the [language reference](language-reference.md) for syntax and
[workflow authoring help](../../docs/help/workflow-authoring.md) for operational commands and
examples.
