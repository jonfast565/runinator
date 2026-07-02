# AGENTS.md

Guidance for agents working in `runinator-command-center`.

## Ownership

`runinator-command-center` is the Tauri/Vue client. Keep UI state, graph editing, workflow run inspection, and API interaction here. Do not change runtime crate behavior from the UI unless the user explicitly asks for a cross-runtime feature.

## Layer Boundaries

The frontend is split for Flutter migration:

| Layer | Path | Rules |
| --- | --- | --- |
| **core/** | `src/core/` | Framework-agnostic TypeScript. No imports from `vue`, `pinia`, `@vue-flow/*`, `@codemirror/*`, `@tauri-apps/*`, or `src/ui/**`. |
| **ui/** | `src/ui/` | Vue presentation: views, components, composables, adapters. Discarded during Flutter migration. |
| **stores shims** | `src/stores/` | Re-export Pinia adapters from `src/ui/adapters/pinia/` for backward-compatible imports. |

### core/ layout

- `core/domain/` — wire models (`models/`, `json.ts`)
- `core/api/` — `commandCenterApi`, `httpRuntime`, injected `CommandRuntime`
- `core/services/` — application logic (`AuthService`, `AppService`, `ResourcesService`, …)
- `core/realtime/` — WebSocket clients and event routing
- `core/navigation/` — tabs, nav config, breakpoints, URL sync helpers
- `core/workflow/` — workflow graph domain logic
- `core/utils/` — pure helpers
- `core/platform/` — platform adapter interfaces

### ui/ layout

- `ui/views/` — screen components (+ `*.manifest.ts` per view for AI migration)
- `ui/components/` — shell, shared, workflow widgets
- `ui/composables/` — thin Vue lifecycle glue over core clients
- `ui/adapters/pinia/` — Pinia stores mirroring core services
- `ui/adapters/vue-flow/` — graph canvas adapter
- `ui/adapters/codemirror/` — editor extensions
- `ui/adapters/browser/` — DOM file/download helpers, HTTP runtime
- `ui/adapters/tauri/` — desktop invoke bridge

## Where To Start

- App shell: `src/App.vue`, `src/ui/components/shell/`
- API facade: `src/core/api/commandCenterApi.ts`, runtime bootstrap in `src/main.ts`
- Services registry: `src/core/services/index.ts`
- Pinia adapters: `src/ui/adapters/pinia/`
- Workflow graph: `src/core/workflow/`, canvas `src/ui/components/workflow/WorkflowCanvas.vue`
- View manifests: `src/ui/views/*.manifest.ts`
- Rust/Tauri commands: `src-tauri/src/`

## Boundaries

- Keep model compatibility with API payloads in `src/core/domain/models/`.
- Put business logic in `src/core/services/` or pure `src/core/utils/`; Pinia adapters stay thin.
- Graph transforms belong in `src/core/workflow/`; Vue Flow rendering in `src/ui/adapters/vue-flow/`.
- Browser-only helpers live in `src/ui/adapters/browser/files.ts`.
- Do not duplicate backend validation rules; client validation is fast feedback only.

## Verification

```bash
pnpm --dir runinator-command-center lint
pnpm --dir runinator-command-center format:check
pnpm --dir runinator-command-center test -- --run
pnpm --dir runinator-command-center build
```

`core/**` is guarded by ESLint `no-restricted-imports` against Vue/Pinia/Vue Flow/CodeMirror/Tauri/ui imports.

For visual changes, verify affected flows on desktop-sized and narrow viewports.
