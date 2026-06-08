# runinator-lsp

An editor-agnostic [Language Server](https://microsoft.github.io/language-server-protocol/)
for the Runinator WDL workflow language. It reuses the pure analyzer, completer, and formatter in
`runinator-wdl` plus the API client in `runinator-api`, so any LSP-capable editor gets:

- **Diagnostics** — live parse/semantic errors as you type (`WDL001`–`WDL006`), with a full
  lowering/validation pass on save.
- **Completion** — provider names, action functions, action parameters, and `config.`/`secret.`
  paths, driven by **live** provider/setting metadata fetched from a running web service.
- **Hover** — the WDL error code + summary for the diagnostic under the cursor.
- **Formatting** — `format_str` on the whole document (format-on-save).
- **Apply-on-save** — optionally compile the saved pack client-side and import it into a running
  web service (`POST /packs/import?overwrite=true`), the editor-native equivalent of
  `runinatorctl workflows dev`.

The server communicates over stdio. The VS Code extension additionally bundles, client-side, a
TextMate **syntax highlighting** grammar and scaffolding **snippets** (`workflow`, `task`, `if`,
`for`, `map`, `parallel`, `try`, `match`, `trigger`, …) that work without a running server; the
semantic completion above (live provider/action/parameter names) comes from the server.

## Build

```bash
cargo build -p runinator-lsp --release
# binary at target/release/runinator-lsp
```

## Configuration

| Setting (LSP `initializationOptions`) | Env | Purpose |
| --- | --- | --- |
| — | `RUNINATOR_API_BASE_URL` (default `http://127.0.0.1:8080/`) | service queried for completion metadata |
| `runinator.autoApply` (bool) | — | import the pack on every save |
| `runinator.serviceUrl` (string) | — | service that apply-on-save imports into |

Metadata is refreshed on a timer; when the service is unreachable, completion degrades to
language keywords and apply-on-save reports a graceful error rather than failing the editor.

## Editor setup

### VS Code

A minimal extension lives in [`editors/vscode/`](editors/vscode). Build the server, then from that
directory:

```bash
npm install
npm run compile
```

Launch the Extension Development Host (F5), or package with `vsce package`. Configure
`runinator.serverPath`, `runinator.apiBaseUrl`, `runinator.autoApply`, and `runinator.serviceUrl`
in Settings.

### Neovim (nvim-lspconfig)

```lua
local configs = require("lspconfig.configs")
local lspconfig = require("lspconfig")

if not configs.runinator then
  configs.runinator = {
    default_config = {
      cmd = { "runinator-lsp" },
      filetypes = { "wdl" },
      root_dir = lspconfig.util.root_pattern(".git", "*.wdlp"),
      cmd_env = { RUNINATOR_API_BASE_URL = "http://127.0.0.1:8080/" },
      init_options = { runinator = { autoApply = true, serviceUrl = "http://127.0.0.1:8080/" } },
    },
  }
end

lspconfig.runinator.setup({})
vim.filetype.add({ extension = { wdl = "wdl", wdlp = "wdl", wdls = "wdl" } })
```

### Zed

Add a custom language server pointing `command` at the `runinator-lsp` binary with
`RUNINATOR_API_BASE_URL` in its environment, scoped to the `wdl` language.

## Note on file kinds

Only `.wdl` (the workflow language) is analyzed and completed. `.wdlp` (JSON pack manifest) and
`.wdls` (secrets) are recognized for apply-on-save packaging but are not run through the workflow
grammar.
