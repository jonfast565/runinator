# runinator-rexrap-ide

The editor surface over [`runinator-rexrap`](../runinator-rexrap): **completion** and **hover**.
Both take a source buffer and a cursor byte offset and answer an author-time question —
*what can go here*, and *what is this* — without influencing what a compiled workflow means.

Neither runs during compilation. A buffer that does not parse still gets useful answers:
both fall back to lexical context when the parse tree is unavailable, which is the normal
case mid-keystroke.

## API

```rust
let response = runinator_rexrap_ide::complete_source(RexRapCompletionRequest {
    source, cursor_byte, providers, settings,
});

let hover = runinator_rexrap_ide::hover_source(RexRapHoverRequest {
    source, cursor_byte, providers, settings,
});
```

`providers` and `settings` are the live metadata that makes completion semantic rather than
syntactic: registered provider actions with their parameter and result types, and the
`config.`/`secret.` slots the settings store knows about. With both empty, completion still
offers language constructs, `std.*` intrinsics, node labels, and run-context fields.

## Consumers

| Consumer | Surface |
| --- | --- |
| `runinator-lsp` | `textDocument/completion`, `textDocument/hover` |
| `runinator-ws` | `POST /rexrap/complete`, `POST /rexrap/hover` |
| `runinator-command-center` | Tauri commands backing the REXRAP editor pane |

## Boundary

This crate reads the language core through `parse_document`, `ast`, and
[`runinator_rexrap::analysis`](../runinator-rexrap/src/analysis.rs) — a deliberately short list that
exists only for this crate. An editor feature needing something new from the core gets it
added to `analysis`; do not widen a core module to reach past it.

Everything that determines program *meaning* — grammar, lowering, decompile, format — stays
in the core, so ctl, the worker, and every compile path link the language without linking the
editor.

## Verification

```bash
cargo test -p runinator-rexrap-ide
```
