# AGENTS.md

Guidance for the entire `runinator-rexrap*` family. Read this file for changes in the facade,
syntax, semantic-analysis, codegen, or IDE crates.

## Ownership and Layering

The authoring language is split by compile stage; dependencies never point back upward:

```text
runinator-rexrap                    public facade, unified .rrx container, analysis seam, tests
  ├── runinator-rexrap-codegen      ast <-> WorkflowDefinition
  │     └── runinator-rexrap-sema   namespace, desugar, registry, purity, types, diagnostics
  │           └── runinator-rexrap-syntax   grammar, ast, comments, parser, format, includes, errors
  └── runinator-rexrap-ide          completion and hover through the facade's analysis seam
```

- Syntax is text/AST only and may depend on `runinator-models`, not diagnostics or workflow graphs.
- Sema determines meaning without producing a runtime artifact. `CompileOptions`, `TypePolicy`, and
  `WorkflowSignature` live here because it is the lowest layer that reads them.
- Codegen owns both lowering and decompilation so one crate owns their round-trip contract.
- The facade assembles/re-exports the core. Ordinary consumers should not name a lower crate.
- IDE features use `runinator-rexrap/src/analysis.rs`; add a deliberate facade seam instead of
  reaching from the IDE into a core crate.
- Shared graph validation belongs in `runinator-workflows`. Lower REXRAP crates use
  `runinator-compute` for expression typing and must not gain a `runinator-workflows` dependency.

Items crossing a stage boundary are `pub`; other helpers stay `pub(crate)`. Cross-stage round-trip
and format-idempotence tests live in `runinator-rexrap`, the first crate that sees all stages. All
four core crates emit `RexRapError`; the `REXRAP` dictionary is defined once in
`runinator-rexrap-syntax/src/errors.rs` and re-exported.

## Language Invariants

- `.rrx` is the only authored pack extension. Its container may carry workflow, pipeline,
  settings, package-manifest, and test blocks; `parse_rrx_blocks` separates their front ends.
- The grammar describes valid programs, not every malformed JSON graph. Do not add syntax for
  degenerate structures such as a condition without branches or parallel without a matching join.
- A workflow has one runtime `do { ... }` block beneath its header declarations. `let` is the only
  binding form.
- Outgoing edges use one attached `routes { ... }` section whose arms `continue` to a target.
  `end` and `fail` are generated terminals; named `join` continuations are reached explicitly.
- `compute { ... }` is the pure in-process statement block; it never schedules provider work.
- Step attributes are prefixes. `@id`, `@skip`, `@lock`, and `@deadline` describe the graph node;
  execution attributes include `@timeout`, `@retry`, `@tags`, `@mcp`, `@runner`, `@idempotent`, and
  `@reentry`. Do not introduce a fluent postfix form.
- A compensation clause places its own attributes between `compensate` and its call. Putting them
  after the call reparses them as the next statement's attributes.

Asyncness belongs to a call site, never a callee. A plain call binds `T`; `async <call>` binds
`task[T]`, consumed by `await` or `detach`. `task fn` defines a runtime region that lowering inlines
with substituted parameters and call-site-namespaced labels; plain `fn` remains pure.
`group_async_launches` groups consecutive independent
launches into a parallel fan-out and joins when a handle is first consumed; a single launch remains
an ordinary node.

Header cron triggers and input defaults live in definition metadata. The web-service importer
materializes REXRAP-managed triggers. Pipeline blocks lower to portable `PipelineBundle` values;
the importer resolves names, upserts the pipeline, and reconciles managed chained triggers by
pipeline id. The pipeline itself does not execute.

## Source and Round-Trip Contract

Authored REXRAP source is never persisted: packs contain compiled definitions and the editor shows
decompiled text. `decompile_with_spans` returns source and byte ranges from the same rendering; do
not store source positions in compiled modules or apply offsets to another rendering.

Every structurally representable node kind/field must survive parse, lower, format, and decompile.
Update IDE completion when new surface syntax should be offered to authors. Comment preservation
keys attachment to reliable anchor starts and `own_line`; nested boundaries come from matching
source braces, not pest span ends that include trivia.

## Where to Start

- Grammar/AST/parser/comments/format/errors: `../runinator-rexrap-syntax/src/`.
- Semantic passes, desugaring, registry, purity, types, options: `../runinator-rexrap-sema/src/`.
- Lower/decompile: `../runinator-rexrap-codegen/src/`.
- Public facade/container handling/tests: `src/`.
- Completion/hover: `../runinator-rexrap-ide/src/` through `src/analysis.rs`.

## Verification

```bash
cargo test -p runinator-rexrap
cargo check -p runinator-rexrap-syntax -p runinator-rexrap-sema -p runinator-rexrap-codegen
```

The facade test suite is authoritative for cross-stage contracts. Syntax changes need focused
parser/lowering/decompile coverage, including terse and explicit forms where both exist.
