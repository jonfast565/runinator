# claude-key-refresh

A small macOS host helper that mirrors the **Claude Code OAuth login** out of the
login Keychain into `~/.claude/.credentials.json`, **writing only when the value
changes**.

## Why

On macOS, Claude Code stores its subscription/SSO login in the **login Keychain**
(`Claude Code-credentials`), not in a file. The Runinator containers run Linux,
where the `claude` binary reads the token from `~/.claude/.credentials.json`. So
mounting `~/.claude` into a container (see `deploy/k8s/overlays/local/claude-creds-patch.yaml`)
carries your config but **not your login**.

This tool bridges the gap (Option 3): it copies the Keychain secret into the file
location the container reads, and re-copies it whenever the Keychain value rotates,
so the mounted `~/.claude` stays logged in. It is **macOS-only** and is never built
into a container image.

## Build

```sh
swift build -c release
# binary: .build/release/claude-key-refresh
```

It is also built by the repo build script on macOS (`build.ps1`), purely to compile
it — it is not packaged into any container.

## Usage

Live dashboard (default) — polls every `--interval` seconds and shows status,
counters, the secret's fingerprint (never the secret itself), and a feed of
captured changes:

```sh
claude-key-refresh                 # watch every 15s, TUI dashboard
claude-key-refresh -i 30           # custom interval
```

One-shot, for cron / launchd (writes if changed, exits non-zero on error):

```sh
claude-key-refresh --once
```

Headless watch that logs one line per check (no TUI):

```sh
claude-key-refresh --plain
```

Remove the mirrored credentials file (e.g. before unmounting or to force a clean
reseed):

```sh
claude-key-refresh --clean          # deletes ~/.claude/.credentials.json, then exits
claude-key-refresh --clean -o PATH  # clean a non-default destination
```

Absence is treated as success; it only removes the destination file, never the
Keychain item.

Options: `--service` (default `Claude Code-credentials`), `--account`,
`--output` (default `~/.claude/.credentials.json`), `--interval`, `--once`, `--plain`, `--clean`.

`-h`/`--help` renders a colorized, sectioned help screen (via
[Rainbow](https://github.com/onevcat/Rainbow)); colors are emitted only on a real
terminal and dropped automatically when piped. Parsing itself stays on
swift-argument-parser.

## Change detection

Writes are gated on a SHA-256 of the secret. It writes only when the Keychain value
differs from what was last seen, or when the destination file has drifted out of
sync (e.g. deleted or tampered). The file is written atomically with `0600` perms.

macOS does not expose a reliable change-notification API for data-protection
generic passwords, so this polls and diffs rather than subscribing to an event.

## First-run Keychain prompt

The **first** Keychain read by this binary triggers a macOS "claude-key-refresh
wants to use the login keychain" dialog. Click **Always Allow** once, interactively,
**before** wiring it into cron/launchd — otherwise an unattended `--once` run will
block on the invisible prompt. Rebuilding the binary changes its identity and may
re-prompt; codesign it stably if that becomes a nuisance.
