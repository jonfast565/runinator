# AGENTS.md

Guidance for agents working in `runinator-command-center`.

## Ownership

`runinator-command-center` is the Tauri/Vue client. Keep UI state, graph editing, workflow run inspection, and API interaction here. Do not change runtime crate behavior from the UI unless the user explicitly asks for a cross-runtime feature.

## Where To Start

- App shell and navigation: `src/App.vue`, `src/components/shell/`.
- API facade/runtime selection: `src/api/`.
- Workflow store facade: `src/stores/workflows/index.ts`.
- Workflow store helper/default logic: `src/stores/workflows/helpers.ts`.
- Workflow graph utilities: `src/utils/workflows/index.ts`.
- Workflow canvas/editor components: `src/components/workflow/`.
- Main workflow screen: `src/views/WorkflowsView.vue`.
- Run views and shared tables: `src/views/RunsView.vue`, `src/components/shared/`.
- Rust/Tauri commands: `src-tauri/src/`.

## Boundaries

- Keep generated model compatibility with API payloads in `src/types/models.ts`.
- Prefer pure utilities in `src/utils/` and store orchestration in `src/stores/`.
- Keep graph editing behavior in `src/utils/workflows/` when it is data transformation, and in components only when it is presentation or interaction state.
- Do not duplicate backend workflow validation rules by hand if the web service already validates them; client-side validation should be fast feedback.
- Do not commit `dist/`, Tauri build output, or generated icon artifacts unless the task explicitly changes those assets.

## Verification

Use:

```bash
pnpm --dir runinator-command-center test -- --run
pnpm --dir runinator-command-center build
```

For visual changes, run the app and verify the affected workflow on desktop-sized and narrow viewports.
