# AGENTS.md

Guidance for the `runinator-ctl`, `runinator-ctl-core`, and `runinator-ctl-wasm` family and the
Command Center surface derived from it.

## Ownership

`runinator-ctl-core` owns the clap command tree and portable console language. Native
`runinatorctl`, its full-screen console, MCP stdio server, and browser WASM validation all derive
from that one command surface. Never add a second verb table, parser, usage model, validator,
completer, or hand-written MCP schema.

Normal CLI and console `:` commands dispatch through the same `src/commands/repl.rs` parse and
`commands::run_command` path. `catalog.rs` walks clap for help, possible values, defaults, and usage;
only console-local `META_COMMANDS` are declared separately. Browser TypeScript command objects are
execution adapters only. Native process/filesystem/MCP functionality stays native; WASM is not a
PTY and never launches a process.

## Full-Screen Console Invariants

- Command modules keep using ordinary stdout/stderr. `src/tui/capture.rs` redirects both into a pipe
  whose reader appends to the transcript.
- Ratatui draws on the duplicate of original stdout returned by `Capture::install`; drawing through
  `io::stdout()` would capture the UI into its own transcript.
- The reader thread never prints and never stops early. Closing its read end makes a later
  `println!` fail with a broken pipe. Replay the transcript on exit so shell scrollback survives.
- Startup avoids `Terminal::clear` and other operations that wait for a cursor-position reply.
- Handle keyboard scrolling before the editor. Mouse-wheel hit testing and drawing share
  `render::bands` layout arithmetic. `--plain` or non-TTY stdout uses reedline.
- Shared capture/transcript code stays in `src/tui/capture.rs`; descriptor/std-handle replacement
  stays in `src/tui/capture/{unix,windows}.rs`. On Windows, crossterm reaches the active console through
  `CONOUT$`/`CONIN$`; do not draw through a startup duplicate that would follow the wrong screen
  buffer. Set/restore the UTF-8 output code page.

## MCP Invariants

- Advertise one tool per unblocked clap command. `src/commands/mcp/schema.rs` derives names, descriptions,
  properties, JSON types, enums, and defaults from clap.
- `mcp/exec.rs::BLOCKED` is the only list of commands unavailable over MCP; schema filtering and
  execution must read the same list.
- Convert tool input back to argv and use the normal `repl::parse`/`run_command` path.
- Capture command stdout/stderr in a scratch file and answer protocol frames on a duplicate of real
  stdout. A file provides a flush/rewind sync point that a live pipe cannot.
- Keep only stream replacement platform-specific: `dup2` on Unix and `SetStdHandle` on Windows.
  Windows behavior needs execution tests, not a compile check, because `println!` consults the std
  handle on each write.

## Where to Start

- Clap tree and portable catalog: `../runinator-ctl-core/src/`.
- Native dispatch/REPL: `src/commands/repl.rs`, command modules.
- TUI capture/transcript/editor/rendering: `src/tui/`.
- MCP schema/dispatch/capture: `src/commands/mcp/`.
- Browser facade: `../runinator-ctl-wasm/src/`; Command Center adapters under its console core.
- Pack application: `../runinator-pack/AGENTS.md`.

## Verification

```bash
cargo test -p runinator-ctl
cargo test -p runinator-ctl-core
cargo test -p runinator-ctl-wasm
```

Command-tree changes need native console, MCP schema/argv, and WASM catalog/completion coverage.
Capture changes require behavioral tests on Unix and Windows; CI's cross-platform job must continue
running `cargo test -p runinator-ctl`, not just compiling it.
