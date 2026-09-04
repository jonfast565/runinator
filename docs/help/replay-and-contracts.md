# Replay safety and published workflow contracts

## Review before replay

`GET /workflow_runs/{id}/replay-plan?from_step_id=<node>` requires Run permission on the source
workflow and performs no mutations. Omit `from_step_id` to restart from the beginning. The plan
contains the original frozen definition, the restart point, ordered ancestor receipt IDs, actions
that may execute, declared and historically resolved idempotency keys, reasons, and a verdict:

- `safe`: no external effects were identified in the conservative execution set.
- `review`: actions or orchestration behavior may repeat. A declared key is evidence, not a
  guarantee: key evaluation, reservation retention, and provider behavior can change.
- `blocked`: the original snapshot/module/configuration is missing, or the selected prefix cannot
  be reconstructed safely.

The Command Center displays this plan for ordinary, selected-node, bulk, console, development-pack,
and emergency replays. Review requires a checked acknowledgement. A blocked plan cannot be started.

The existing `POST /workflow_runs/{id}/replay` remains the only replay mutation. Send
`from_step_id`, `plan_fingerprint`, and `acknowledge_review: true` after reviewing a `review` plan.
The server recomputes the plan and refuses a stale fingerprint. Old callers without a fingerprint
remain compatible only with `safe` plans. Managed-orchestration `override_reason` and
`idempotency_key` are separate controls; neither substitutes for replay acknowledgement.

```sh
runinatorctl runs replay-plan RUN_UUID --from-step publish
runinatorctl runs replay RUN_UUID --from-step publish --acknowledge-review
```

Selected-step replay interprets the frozen bytecode prefix locally, feeding it unique successful
action receipts whose requests match the reconstructed inputs. It atomically installs the resulting
locals and stack into a fresh root continuation; provenance records the receipt IDs and reviewed
fingerprint. It never manufactures old effect or node-run rows, and it never dispatches prefix work.
The new run retains the original module and configuration rather than reading the current workflow
or settings. Stateful ancestry (loops, maps, parallel/race, try), ambiguous/repeated receipts,
orchestration-effect ancestry, interrupt handlers, and nondeterministic local calls are conservatively
blocked. The may-execute list can include branches or compensations that will not actually run.

## Publish an I/O contract

REXRAP `returns T` now writes `WorkflowDefinition.output_type`. Omitting `returns` publishes `Any`
(unknown); this release does not infer a return shape. Existing stored workflow signatures expose
their published output type to REXRAP subflow checking. Definitions, compiled packs, exports,
revisions, and revision restores carry both input and output contracts.

Legacy JSON without `output_type`, and legacy database rows whose `output_schema` is null, recover
the old declaration from `definition.metadata.rexrap.output_type`. Otherwise they use `Any`.
An explicit `Any` is authoritative. Historical revision digests and pins are not rewritten; newly
published revisions use an output-aware, domain-separated SHA-256 digest with the existing
`sha256:` wire prefix.

Compatibility is structural and conservative:

- New inputs must accept every input accepted before (contravariance).
- New outputs must satisfy the previous return contract (covariance).
- Required/optional fields, open records, arrays/maps, unions, numeric ranges/enums, and function
  parameter/result variance participate. Unknown output becoming known can narrow safely;
  replacing a known output with `Any` is breaking.

`POST /workflows/{id}/contract-impact` accepts a proposed workflow definition without saving it.
It returns compatibility, reasons, the required major-version action, and visible direct subflow
and pipeline consumers. Pinned consumers are identified separately because they keep their immutable
revision. The Command Center revision panel compares both contracts and can preview a restore's
impact against the current head.

A breaking publication must increase the workflow's **major** version, or supply a nonempty
`contract_override_reason` query parameter. Direct saves and restores require workflow Own for an
override. Compiled-pack imports require Own on the import organization/platform scope. Edit/Run
permission, machine status, and replay acknowledgements do not grant override authority.

```sh
runinatorctl workflows apply ./pack --contract-override-reason "Consumers migrated under CHG-42"
```

Head publication, immutable revision, and any override audit entry commit in one transaction.
A stale head comparison fails instead of overwriting a concurrently published contract. Pack imports
retain their outer all-or-nothing transaction. Restore is a forward publication and follows the same
policy. These checks govern publication, not already-running frozen modules, and do not add runtime
return-value validation.
