# Console and MCP

Use this guide to work with REXRAP notebook sessions in the terminal or command center, and to expose the same control surface to an MCP client.

## The REXRAP Console

A notebook of cells sharing one scope, for working out what a workflow should say.

```
POST /console/sessions                 # start a notebook
POST /console/sessions/{id}/cells      # append a cell
POST /console/cells/{id}/run           # run it
```

A cell is a fragment of the same REXRAP a workflow is written in, and it is answered one of two ways.
A **pure** cell — an expression or a `compute` block — is evaluated in process and has already
settled when the request returns. **Anything else** becomes a hidden scratch workflow and goes
through the ordinary graph-runtime path. Classification is conservative and the workflow fallback is
unconditional: a cell wrongly treated as pure would run a provider action inside an HTTP handler,
with no run to record it and no retry, timeout, or cancellation.

Cells share a scope. A cell's result binds under its label (or `cell_<n>` if unlabelled) and a later
cell reads it as `params.<name>`. `params` rather than a console-only root because a bare dotted
path in REXRAP already means *node output* — `cells.load` would be a reference to a node called
`cells`. It is still a namespace, so a cell labelled `config` binds to `params.config` and cannot
shadow the real `config` root. The scope is also what a scratch run receives as its parameters, so a
name means one thing however the cell ran.

The scope lives in the database, not in a replica's memory: a session outlives any one request, and
an in-process scope would give different answers depending on which replica served the cell.

Editing a cell clears its previous result, and a failing cell drops its binding — in both cases
because a stale value shown as a current one is worse than an absent one.

### The two consoles

The console has two front ends, and they are deliberately the same thing: a scrollback of what has
been run, a prompt at the bottom, and the session's scope beside it.

`runinatorctl console` opens it in the terminal, as a full-screen ratatui interface: a status line
naming the session and the service, a scrollable pane holding everything commands have printed, the
input, a completion menu, and a key legend.

The output pane is the console's own scrollback, not the terminal's. `PgUp`/`PgDn` page through it,
`Shift+↑`/`Shift+↓` move a line, `Shift+Home`/`Shift+End` jump to the oldest line or back to
following, and `Shift+←`/`Shift+→` scroll sideways for output wider than the pane (tables are
truncated rather than wrapped, the way `less -S` does it). The wheel scrolls whichever pane the
pointer is over — the output, or the input when a multi-line cell is taller than the four rows it
gets. `↑`/`↓` remain history recall, and typing anything puts the input pane back under the caret.

A pane that has been scrolled back stays where it was put while a command keeps printing, and says
so in its header rather than looking live. Output arriving during a long run reaches the pane as it
happens, so a run can be read while it is still going, and `Ctrl+C` interrupts the wait without
leaving the console. Quitting replays the session's output to the terminal, so the shell's own
scrollback ends up holding what it would have held anyway.

`--plain` falls back to the single-line reedline prompt, which is also what a pipe gets
automatically. The Console tab in the command center is the same layout in the browser.

The console runs on Linux, macOS, and Windows. Taking stdout away from the command modules is the
one per-platform part — `dup2` on a descriptor, `SetStdHandle` on a std handle — and crossterm is
what makes the Windows half work: it reaches the terminal through `CONOUT$` and `CONIN$`, opened by
name, so the size query, raw mode, the alternate screen, and the event source cannot see the
redirection at all. The console also puts the Windows console into UTF-8 for the duration and puts
the code page back on the way out, since it draws through a handle that carries bytes rather than
through Rust's own `Stdout`.

In both, a **bare line is REXRAP** and becomes a durable cell; a **`:` line is a command**. Every
`runinatorctl` command works with a `:` in front of it — `:runs list --open`, `:settings get aws
key`, `:agents drain <replica>` — because the terminal console hands the line to the same clap
parser the process uses rather than keeping a second table of verbs. The web console implements the
same vocabulary over the HTTP API; the handful of commands that read or write a working tree
(`workflows apply`, `functions publish`, `settings import`, `artifacts download`) stay listed in
`:help` and say to run them with `runinatorctl`.

`:help` prints one table of every command against what it does, and `:help <command>` narrows to a
prefix or expands one command into its call shape and each argument. The list is *derived* — the
terminal console walks the same clap tree the process parses with, so a verb added to the CLI is
listed, completed, and explained the day it is added.

Both consoles read a line the same way: tokenize, take the longest command path that prefixes the
tokens, then split what is left into positionals and flags (`--name value`, `--name=value`, and a
bare switch all work). A flag the command does not take is an error naming the ones it does, and an
unknown verb suggests the nearest one rather than only refusing.

`Tab` completes verbs, subcommands, long flags, and the **values** a flag accepts when they are a
closed set (`:replicas list --status ` offers `live`, `stale`, `offline`). When there is nothing to
insert, the band under the prompt says what belongs there instead — `<workflow>`, `--kind <KIND>`.
`Enter` runs a finished line and opens a new one while a brace, bracket, paren, or quote is still
open, so a multi-line workflow can be typed straight into the prompt and executed as a scratch run.

A few verbs exist only inside a session, since they have no command-line counterpart: `:sessions`,
`:new`, `:use`, `:history`, `:bindings`, `:cancel`, `:replay`, `:run workflow|pipeline`, `:invoke`,
and `:clear`. `:run` and `:invoke` take their payload either way — `--param KEY=VALUE` or a
`… with {"a": 1}` tail — so a line copied between the two consoles keeps working.

### The MCP server

`runinatorctl mcp` is the same control surface again, for a model rather than a person: a Model
Context Protocol server speaking json-rpc on stdin and stdout. It is meant to be launched by an MCP
client, not run by hand.

```jsonc
// claude_desktop_config.json, .mcp.json, or whatever your client reads
{
  "mcpServers": {
    "runinator": {
      "command": "runinatorctl",
      "args": ["mcp", "--api-base-url", "http://127.0.0.1:8080"],
      "env": { "RUNINATOR_API_KEY": "…" }
    }
  }
}
```

**Every `runinatorctl` command is a tool**, named `runinator_<command>_<subcommand>` —
`runinator_workflows_apply`, `runinator_runs_show`, `runinator_settings_set`, and eighty-odd more.
Each one's description, arguments, types, closed sets, and defaults are read out of the same clap
tree the process parses its own argv with, so a verb added to the CLI is a tool with a correct
schema the day it is added; nothing is written down twice. A call is turned back into argv and run
through the ordinary parser and dispatch, so there is one execution path, not two.

Two tools sit in front of that set. `runinator_help` is the index — the `:help` table, for finding a
verb without pulling ninety schemas into the conversation. `runinator_exec` runs a raw command line,
which is the escape hatch for a longer timeout, for output as a table rather than json, or for
anything a schema does not express.

Runs, their logs, and their artifacts are also readable as **resources** (`runinator://runs/{id}`,
`runinator://node_runs/{id}/chunks`, …), so a client can attach what a run left behind to the
conversation without spending a tool call on it. `--workflow-tools` additionally exposes every
enabled workflow as a tool that starts a run of it, typed by the workflow's own declared input; it
is off by default, because a fleet of workflows would bury the commands that author them.

The verbs that never return or read the terminal are refused rather than left to hang the call —
`console`, `mcp`, `login`/`logout`, `workflows dev`, `runs watch` — each naming what to do instead.
Two commands that need no server (`workflows test`, `functions validate`) run offline, which is when
a dry run is most useful. The server also starts against a web service that is not up yet: a client
launches it before the stack is running, so an unreachable service becomes an error on the first
tool call rather than a process that exits at startup.

Command output is captured underneath the command modules, the way the terminal console captures it,
because they print with plain `println!` and a table written into the middle of a json-rpc frame
would desynchronise the client. Moving a standard stream is the one per-platform part —
`dup2` on a descriptor, `SetStdHandle` on a console handle — and it is the whole of
`capture/unix.rs` and `capture/windows.rs`; everything above that line is shared. The server runs on
Linux, macOS, and Windows alike.

When the web service lives in kubernetes rather than on localhost, `scripts/start-runinatorctl.sh
--mcp` is the launcher to point the client at: it brings up the same port-forward the console uses,
signs in if the service enforces auth, and then runs `runinatorctl mcp` against it, tearing the
forward down when the client disconnects. Every message the script prints moves to stderr under
`--mcp`, since stdout carries the protocol. Arguments after the script's own flags go to the
subcommand, so `scripts/start-runinatorctl.sh --mcp --workflow-tools` works as expected.
