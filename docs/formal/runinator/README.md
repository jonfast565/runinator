# Runinator Alloy model

This directory contains a bounded Alloy 6 temporal model of Runinator's trust boundaries and
durable workflow lifecycle. It is an executable architecture specification, not a replacement for
Rust tests or a proof over unbounded executions.

## Layout

| Module | Responsibility |
| --- | --- |
| `types.als` | actors, channels, statuses, and immutable categories |
| `topology.als` | permitted message directions across the control and execution planes |
| `durable_state.als` | durable runs, continuations, effects, dispatches, admissions, and artifacts |
| `control_plane.als` | authorized control-plane start and denied-request trace |
| `vm_lifecycle.als` | continuation boundaries, fork, completion, and run settlement |
| `effect_delivery.als` | outbox publication, redelivery, worker results, settlement, and retry |
| `ingress.als` | timer wake relay and agent ingress |
| `artifacts.als` | blob persistence before artifact recording |
| `properties.als` | cross-domain safety assertions |
| `scenarios.als` | concrete temporal traces |
| `runinator.als` | composition root, trace rule, and Analyzer commands |

The mutable state lives only in `durable_state.als`. Domain modules define named transitions that
frame every state relation they do not modify. `runinator.als` is the only file with `run` and
`check` commands.

## Correspondence to the implementation

The model follows the architecture described in [`../../architecture.md`](../../architecture.md):

- `WebService` represents the authenticated HTTP/WebSocket control surface; `authorizedStart`
  abstracts its authorization and repository delegation.
- `Engine` represents `runinator-engine` plus the `WorkflowVmHost` transaction boundary.
- `DurableStore` represents the `WorkflowVmStore`/runtime-store contracts and their database
  implementations.
- `Dispatch`, `Effect`, and `Continuation` model the durable outbox and continuation records.
- `Worker`, `Waker`, `AdapterHost`, and `Agent` can communicate with orchestration only on their
  documented broker channels.

Workflow bytecode, provider behavior, SQL details, wire payload values, broker implementation, and
artifact bytes are intentionally abstract. The model instead checks the ownership and lifecycle
rules that must survive retries and redelivery.

## Running it

1. Install or open the [Alloy 6 Analyzer](https://alloytools.org/).
2. Open `runinator.als` with `docs/formal` as the project root, so imports such as
   `runinator/durable_state` resolve to this directory.
3. Execute the six named `run` commands to inspect satisfying traces.
4. Execute every named `check` command. The documented scopes are intentionally small and should
   be treated as regression scopes; increase scopes and trace lengths when investigating a change.

The scenarios cover a successful action, timer wake relay, action redelivery, retry, and a denied
unauthorized request. The checks focus on unique logical effects,
single-settlement behavior, parked-continuation integrity, terminal-run gating, authorized
admission, ingress-only timer settlement, and durable artifact recording.

## Maintaining the model

When a Runinator change affects an architectural invariant, update the module that owns the
corresponding boundary and add or adjust a scenario/check in the root. Before relying on a new
assertion, briefly weaken the relevant transition or fact and confirm the Analyzer finds the
expected counterexample. Useful mutations include allowing duplicate effect creation, permitting a
terminal run with a runnable continuation, recording an artifact before its blob, or settling a
timer without ingress.

## Counterexample handling

Treat a counterexample as a model/code comparison task. Change Rust only when the trace can be
mapped to a reachable behavior in the corresponding runtime, engine, or store contract and is
reproduced by a focused Rust test. If the trace relies on an abstraction that the code does not
implement, correct the Alloy model instead. This keeps the specification grounded in the codebase
rather than turning modeling assumptions into product behavior.
