# AGENTS.md

Guidance for agents working in the `runinator-wdl` crate family.

## Ownership

The WDL family owns the author-time workflow language: grammar, parser, semantic diagnostics, lowering to JSON workflow definitions, formatting, desugaring, and decompilation. Runtime execution belongs to `runinator-ws`, validation shared with JSON workflows belongs in `runinator-workflows`, and the editor surface (completion, hover) belongs in `runinator-wdl-ide`.

## The Four Crates

The language core is split by compile stage, layered so nothing depends back up:

```
runinator-wdl                     public api, `.wdlp`/`.wdls` front ends,
                                  `analysis` seam, the test suite
  ├── runinator-wdl-codegen       lower/  (ast -> json model)
  │                               decompile/ (json model -> text)
  │     └── runinator-wdl-sema
  ├── runinator-wdl-sema          namespace, desugar, registry, purity, types,
  │                               sema/, options
  │     └── runinator-wdl-syntax
  └── runinator-wdl-syntax        errors, ast, comments, parser, format, includes
```

Pick a crate by compile stage, not by file size:

- `runinator-wdl-syntax` is text ↔ ast and nothing else. It knows no diagnostics, no workflow JSON model, and no provider metadata; `runinator-models` is its only runinator dependency. Anything needing a `Diagnostic` or a `WorkflowDefinition` does not belong here.
- `runinator-wdl-sema` answers what a program *means* without producing a runtime artifact. `CompileOptions`/`TypePolicy`/`WorkflowSignature` live here because this is the lowest layer that reads them — the type passes need the policy and the pack's signatures, and codegen needs the rest.
- `runinator-wdl-codegen` holds both directions of the ast ↔ JSON mapping. `lower` and `decompile` share no code (decompile emits text directly rather than building an ast) but they share a contract: every node kind's parameters must survive a round trip. One crate, one owner for that contract.
- `runinator-wdl` assembles them. It is the only crate every consumer links, and it re-exports `ast`, `comments`, `errors`, `sema`, `CompileOptions`, and friends at their historical `runinator_wdl::…` paths — a consumer should never need to name a core crate directly.

The **round-trip and format-idempotence tests live in `runinator-wdl`**, because it is the first crate that can see parse, lower, decompile, and format at once. That is deliberate: those contracts are cross-stage, so they cannot be pinned from inside any one stage.

Items crossing a crate boundary are `pub`; everything else stays `pub(crate)`. When adding a helper, default to `pub(crate)` and widen only when a higher crate actually needs it — the compiler will say so.

The **error dictionary is not per-crate here**. All four crates emit the same `WdlError` with the same `WDL` prefix, so `DICTIONARY` is defined once in `runinator-wdl-syntax`'s `errors.rs` and re-exported by `runinator-wdl`. Add a new numbered code there, not to a per-crate dictionary.

## Where To Start

`runinator-wdl-syntax`:

- Grammar: `src/wdl.pest`.
- AST and parser: `src/ast.rs`, `src/parser.rs`.
- Comment preservation: `src/comments.rs`. Pest treats `COMMENT` as silent trivia, so comments are lexed separately and attached to ast anchors (as `CommentSet` leading/trailing/dangling) after parsing; the formatter renders them back for lossless round-trips. Anchors include top-level items, workflow headers, body statements (recursively, per branch), and `params`/`type` struct fields (recursively). Attachment keys off reliable anchor *start* positions and the `own_line` flag, not pest `span.end` (which includes trailing trivia); nested-block and struct boundaries are found by matching source braces (`block_close`).
- Formatting: `src/format.rs`.
- Errors and spans: `src/errors.rs`.

`runinator-wdl-sema`:

- Semantic passes: `src/sema/`.
- Desugaring: `src/desugar.rs`; namespace resolution: `src/namespace.rs`.
- Callable registry and purity: `src/registry.rs`, `src/purity.rs`.
- Named-type resolution: `src/types.rs`.
- Compile options: `src/options.rs`.

`runinator-wdl-codegen`:

- Lowering to workflow JSON: `src/lower/`.
- Decompilation: `src/decompile/`.

`runinator-wdl`:

- Public compile/decompile facade: `src/lib.rs`.
- Editor seam: `src/analysis.rs` — the only items `runinator-wdl-ide` may reach into `runinator-wdl-sema` for. Add to it deliberately rather than reaching into a core crate.
- `.wdlp` pipelines and `.wdls` secrets: `src/pipeline.rs`, `src/secrets.rs`.
- Regression coverage: `src/tests/`, one file per subject; shared round-trip helpers in `tests/mod.rs`.

## Boundaries

- Keep the grammar a syntax for valid authoring forms, not a serializer for every malformed JSON graph.
- New node kinds or fields must round-trip through parse, lower, format, and decompile when structurally representable; update `runinator-wdl-ide` completion when the surface gains something an author would want offered.
- Use `runinator-workflows` validation after lowering; do not duplicate shared graph invariants here unless they are language-specific diagnostics. Note the asymmetry: only the `runinator-wdl` facade needs that graph layer. `-wdl-sema`, `-wdl-ide`, and `-wdl-codegen` read the intrinsic catalog and expression typing from **`runinator-compute`** and must not gain a `runinator-workflows` dependency — a semantic pass reaching for graph validation is a sign the check belongs in the facade's lowering path instead.
- Do not add runtime scheduling, broker, database, worker, or provider behavior to these crates.
- Do not add a dependency from a lower crate back up (syntax must never name sema; sema must never name codegen). If a pass seems to need one, the pass is in the wrong crate.

## Verification

Use:

```bash
cargo test -p runinator-wdl
cargo check -p runinator-wdl-syntax -p runinator-wdl-sema -p runinator-wdl-codegen
```

`cargo test -p runinator-wdl` is the one that matters — the cross-stage contracts (round-trip, format idempotence) are all in that suite. For syntax changes, add parser/lowering/decompile tests that cover both terse and explicit forms when applicable.
