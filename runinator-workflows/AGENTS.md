# AGENTS.md

Guidance for `runinator-workflows` and its lower-level expression dependency `runinator-compute`.

## Ownership

`runinator-compute` evaluates references/templates, declarative conditions, compute programs,
intrinsics, user functions, and argument-dependent result types. It knows nothing about workflow
nodes, transitions, or graph validation.

`runinator-workflows` owns graph validation, cycle/region checks, the node-kind catalog, graph type
checking, compilation, and simulation. It re-exports compute at historical paths for graph
consumers; expression-only consumers depend on `runinator-compute` directly.

## Node-Kind Invariants

- Each `WorkflowNodeKind` has one `NodeKindSpec` file under its catalog category in
  `src/node_kinds/`. The spec owns palette metadata, `GraphRole`, parameter-carried `TargetSlot`s,
  parameter shape validation, and statically known output type.
- Add a kind by adding its file, category `mod`/`pub(super) use`, and exhaustive `spec_for` arm.
  Catalog, parameters, validation, typing, and simulation read shared facts from the spec rather
  than growing parallel metadata matches.
- Per-kind type checks stay in `typing.rs` because they require private inference context. Per-kind
  simulation stays in `simulate.rs` because it requires private outcome/environment types. Both
  remain exhaustive and consume registry facts; do not widen private types merely to move bodies.
- Catalog `edge_slots` and spec `target_slots` are two views of the same edges. Keep
  `src/node_kinds/tests.rs` parity in both directions so the UI never advertises an edge the runtime
  ignores.
- `GraphRole::handler_safe` defaults to false. Interrupt-region validation permits only opted-in
  kinds and enforces isolated single-entry/single-exit regions whose paths end at `resume`.
- Grammar-specific diagnostics belong to REXRAP, but graph invariants shared with JSON definitions
  belong here. Runtime transition behavior belongs in `runinator-runtime`.

`runinator-compute` and `runinator-workflows` emit the same `WorkflowValidationError`; the shared
`WORKFLOW` dictionary lives once in `runinator-compute/src/errors.rs` and is re-exported. Do not add
a second dictionary to the graph wrapper.

## Where to Start

- Node catalog/specs: `src/node_kinds/`.
- Graph validation/types/simulation: `src/validation.rs`, `src/typing.rs`, `src/simulate.rs`.
- Expression VM/intrinsics/errors: `../runinator-compute/src/`.
- Authoring language integration: `../runinator-rexrap/AGENTS.md`.
- Runtime behavior: `../runinator-runtime/AGENTS.md`.

## Verification

```bash
cargo check -p runinator-compute -p runinator-workflows
cargo test -p runinator-compute
cargo test -p runinator-workflows
```

New node kinds need catalog/target parity, parameter, type, simulation, and runtime tests plus REXRAP
round-trip coverage when authorable.
