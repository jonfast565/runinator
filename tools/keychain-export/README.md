# keychain-export

A tiny macOS-only host helper that reads a secret from the login Keychain and
emits it. It defaults to the **Claude Code OAuth login** (a generic password
under `Claude Code-credentials`, raw bytes to stdout) so the rotator can capture
it, but it can extract **any generic- or internet-password item to any path** in
`raw`/`base64`/`hex`/`json` form. It does one thing — native Keychain extraction
— and has **no external dependencies**.

## Why

On macOS, Claude Code stores its subscription/SSO login in the **login Keychain**,
not in a file. Linux containers read the token from a file
(`~/.claude/.credentials.json`). This tool is the "get the key" piece: it pulls a
secret out of the Keychain and emits it on stdout, so a generic sync engine like
the companion [`runinator-secret-sync`](../runinator-secret-sync) can wire it into
a job (`source: command [keychain-export …]`) and deliver it to a Kubernetes
Secret or file. It is **macOS-only** and is never built into a container image.

## Build

```sh
swift build -c release
# binary: .build/release/keychain-export
```

It is also compiled by the repo build script on macOS (`build.ps1`), purely to
compile-check it — it is not packaged into any container.

## Usage

```sh
# default: the Claude login as raw bytes on stdout
keychain-export

# any item, written to any path, in a chosen format
keychain-export -s "my-service" --account alice -o secret.txt
keychain-export -s "my-service" -f base64 -o secret.b64
keychain-export -s "my-service" -f json   -o secret.json
keychain-export -s "api.example.com" --kind internet -f json

keychain-export -q                 # suppress the stderr fingerprint line
```

The secret itself is never printed to stderr — only a short SHA-256 fingerprint
(unless `-q`). The actual value goes to stdout or the `--output` file.

Options:

| flag | default | meaning |
|------|---------|---------|
| `-s, --service <name>` | `Claude Code-credentials` | Keychain service (generic) or server (internet) |
| `--account <name>` | — | account to disambiguate the item |
| `--kind <kind>` | `generic` | item class: `generic` or `internet` password |
| `-f, --format <fmt>` | `raw` | `raw` (bytes unchanged), `base64`, `hex`, or `json` |
| `-o, --output <path>` | stdout | write atomically to a `0600` file |
| `-q, --quiet` | off | suppress the stderr fingerprint line |

`raw` emits the secret bytes verbatim (e.g. the credential JSON itself). `json`
wraps it as `{"service","account","value","encoding"}` — `value` is the UTF-8
string when the bytes decode cleanly, otherwise base64 with `"encoding":"base64"`.
Text formats (`base64`/`hex`/`json`) get a trailing newline; `raw` stays
byte-exact, which is what the rotator consumes.

Exit codes: `0` wrote a credential, `3` the Keychain item was not found (e.g.
Claude Code is not logged in), `1` any other error. The rotator keys off exit
code `3` to report a missing login distinctly.

## First-run Keychain prompt

The **first** Keychain read by this binary triggers a macOS "keychain-export
wants to use the login keychain" dialog. Click **Always Allow** once,
interactively, **before** wiring it into an unattended rotator run — otherwise the
invisible prompt will block. Rebuilding the binary changes its identity and may
re-prompt; codesign it stably if that becomes a nuisance.
