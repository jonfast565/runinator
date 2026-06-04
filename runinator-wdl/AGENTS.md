# AGENTS.md

Guidance for agents working in `runinator-wdl`.

## Ownership

`runinator-wdl` owns the author-time workflow language: grammar, parser, semantic diagnostics, lowering to JSON workflow definitions, formatting, completion, desugaring, and decompilation. Runtime execution belongs to `runinator-ws` and validation shared with JSON workflows belongs in `runinator-workflows`.

## Where To Start

- Grammar: `src/wdl.pest`.
- AST and parser: `src/ast.rs`, `src/parser.rs`.
- Semantic passes: `src/sema/`.
- Lowering to workflow JSON: `src/lower/`.
- Desugaring: `src/desugar.rs`.
- Formatting and completion: `src/format.rs`, `src/completion.rs`.
- Decompilation: `src/decompile/`.
- Public compile/decompile facade: `src/lib.rs`.
- Regression coverage: `src/tests.rs`.

## Boundaries

- Keep the grammar a syntax for valid authoring forms, not a serializer for every malformed JSON graph.
- New node kinds or fields must round-trip through parse, lower, format, completion if applicable, and decompile when structurally representable.
- Use `runinator-workflows` validation after lowering; do not duplicate shared graph invariants here unless they are language-specific diagnostics.
- Do not add runtime scheduling, broker, database, worker, or provider behavior to this crate.

## Verification

Use:

```bash
cargo test -p runinator-wdl
cargo check -p runinator-wdl
```

For syntax changes, add parser/lowering/decompile tests that cover both terse and explicit forms when applicable.
