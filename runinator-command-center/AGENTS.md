# AGENTS.md

Guidance for `runinator-command-center`, the Tauri/Vue control client.

## Ownership

The command center edits packs, calls the web service, and presents runtime state. It never hosts a
worker, loads providers, or exposes worker lifecycle APIs; machine-local execution belongs to the
standalone `runinator-desktop-agent`.

The frontend has a framework-agnostic core and a Vue presentation layer:

| Layer | Path | Rule |
| --- | --- | --- |
| Core | `src/core/` | No Vue, Pinia, Vue Flow, CodeMirror, Tauri, or `src/ui/` imports. |
| UI | `src/ui/` | Views, components, composables, and framework/platform adapters. |

Business logic belongs in `src/core/services/` or pure `src/core/utils/`; Pinia adapters remain thin.
Graph transforms live in `src/core/workflow/`, Vue Flow rendering in `src/ui/adapters/vue-flow/`, browser
file/download helpers in `src/ui/adapters/browser/`, and desktop invokes in `src/ui/adapters/tauri/`.
Bootstrap selects the platform adapter and registers the CodeMirror text-editor factory before
mounting.

## Area Wiring

Functions, Console, and Orchestrations use the same vertical slice:

- Wire/domain model under `src/core/domain/models/<area>/` and its central export.
- Service under `src/core/services/` and `src/core/services/index.ts`.
- API method plus `src/core/api/httpRuntime.ts` registry entry.
- Pinia adapter under `src/ui/adapters/pinia/` and view under `src/ui/views/`.
- Navigation tab/config, `App.vue` wiring, and matching Tauri command/`generate_handler![]` entry
  where desktop bridging is required.

Do not duplicate backend validation; client validation is fast feedback only. Keep API payloads
compatible with `src/core/domain/models/`.

## Console and Function Publishing

The browser console derives parsing, validation, help, completion, and multiline readiness from
`runinator-ctl-wasm`. TypeScript command objects are execution adapters, not a second command
registry or tokenizer. Read `runinator-ctl/AGENTS.md` for the shared command-surface contract.
Interactive process terminals remain worker PTY/ConPTY streams rendered by xterm; WASM is not a
PTY and never launches processes.

The Functions publish dialog uploads an archive the operator already built plus its manifest and
computes the SHA-256 from uploaded bytes. A browser has no working tree to archive. `src/core/utils/zip.ts`
may recover the manifest from a walkable ZIP and otherwise leaves it for manual entry. Do not make
the browser path imitate `runinatorctl functions publish`, which deterministically archives a
directory.

## Where to Start

- App shell: `src/App.vue`, `src/ui/components/shell/`.
- API/runtime: `src/core/api/`, bootstrap in `src/bootstrap.ts`.
- Services and Pinia adapters: `src/core/services/`, `src/ui/adapters/pinia/`.
- Workflow graph: `src/core/workflow/`, workflow services, and `src/ui/components/workflow/`.
- Rust/Tauri commands: `src-tauri/src/`.

## Verification

```bash
pnpm --dir runinator-command-center lint
pnpm --dir runinator-command-center format:check
pnpm --dir runinator-command-center test -- --run
pnpm --dir runinator-command-center build
```

ESLint guards `src/core/**` imports. For visual changes, verify desktop-sized and narrow viewports.
Workspace-wide Cargo commands unify features with the desktop agent. Keep its `rfd` feature set
compatible with `tauri-plugin-dialog`; enabling both Linux `gtk3` and `xdg-portal` backends makes
the `rfd` build script fail.
