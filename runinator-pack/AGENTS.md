# AGENTS.md

Guidance for `runinator-pack` and sibling `runinator-pack-wire`.

## Ownership

Pack compilation is client-side. `runinator-pack` compiles a `.rrx` file/directory into portable
workflow/settings/pipeline artifacts and builds function-package archives. `runinator-pack-wire`
alone owns compiled-pack ZIP entry names and read/write behavior shared by ctl, API, command center,
and web-service import.

`runinatorctl workflows apply` compiles locally, writes a ZIP, and uploads it to `/packs/import`.
The backend reads compiled JSON and never compiles authored source. Writers emit `workflows.json`
plus optional versioned `settings.json` and `pipelines.json`; readers may accept the legacy
`secrets.json` compatibility entry, but writers never emit it.

## Archive and Binding Invariants

- Function-package archives are content-addressed and deterministic. Fix entry order, timestamps,
  permissions, and compression method; do not let mtimes, executable bits, or directory iteration
  affect the digest.
- `functions.<package>.<export>(...)` needs no grammar special case. Lowering maps it to provider
  `functions`, action `invoke`, nests authored args under `configuration.input`, and attaches a
  `FunctionBinding` that pins version/digest.
- Apply function bindings in both ordinary action lowering and action-object/compensation lowering.
  Decompile from the binding without requiring a live catalog so deleted packages do not change
  rendered source.
- Synthetic function providers participate in callable typing but do not make
  `provider_catalog_present` true; offline compilation remains permissive unless a real provider
  catalog is present.

## Where to Start

- Source/pack compilation and function archives: `src/`.
- Deterministic function ZIP logic: `src/functions/archive.rs`.
- Wire layout: `../runinator-pack-wire/src/`.
- REXRAP lowering/decompile rules: `../runinator-rexrap/AGENTS.md`.
- CLI upload path: `../runinator-ctl/`.

## Verification

```bash
cargo check -p runinator-pack -p runinator-pack-wire
cargo test -p runinator-pack
cargo test -p runinator-pack-wire
```

Archive tests must prove identical bytes/digests from equivalent trees. Pack changes need writer
and web-service reader compatibility plus REXRAP round trips when bindings change.
