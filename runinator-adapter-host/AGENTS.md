# AGENTS.md

Guidance for the `runinator-adapter-*` family.

## Ownership

- `runinator-adapter-contract` owns the file ABI symbols, request/response payloads, and shared
  bearer/HMAC verification. It has no transport dependency.
- `runinator-adapter-sdk` is the out-of-tree implementation surface; `poll` has a default so
  webhook-only adapters remain source-compatible.
- `runinator-adapter-host` loads and executes adapter code over loopback HTTP. Each dynamic
  invocation runs in a disposable child process.
- `runinator-adapter-client` owns URL/token discovery and the authenticated HTTP protocol used by
  both authoring handlers and the engine. Callers must not re-derive
  `RUNINATOR_ADAPTER_HOST_URL`/`RUNINATOR_ADAPTER_HOST_TOKEN`.

## Invariants

- Inbound webhook/poll behavior belongs here. Outbound actions belong to
  `runinator-provider-*`; provider GitHub/Jira clients are not interchangeable with built-in
  adapter pollers.
- The host listens on loopback only because it executes adapter libraries. It ships as a sidecar of
  web-service and engine-worker pods, never as a Kubernetes Service.
- Static container builds have no dynamic loader and serve only compiled-in adapter kinds. Do not
  design a container path that assumes arbitrary shared-library loading.
- Keep authentication/canonicalization in the contract and client discovery in the client so
  authoring and durable polling cannot drift.

## Where to Start

- ABI/auth payloads: `../runinator-adapter-contract/src/`.
- SDK surface: `../runinator-adapter-sdk/src/`.
- Host loading/built-ins/process isolation: `src/`.
- Client/discovery: `../runinator-adapter-client/src/`.
- Callers: authoring adapter handlers and engine polling services.

## Verification

Run focused checks for every changed family crate. Contract changes require SDK, host, client,
authoring, and engine compatibility tests; loading changes require disposable-process and loopback
behavior coverage.
