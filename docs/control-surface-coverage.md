# Control-surface coverage

Runinator treats operator accessibility as a versioned contract. Every authenticated operator API
operation is assigned to one capability, and every capability records how Command Center answers
five requirements:

- **Discoverable:** an operator can find the capability and understand its purpose.
- **Configurable:** mutable policy has a typed surface and authoritative validation.
- **Observable:** current state, history, failures, and provenance are visible.
- **Controllable:** safe operational actions are exposed with backend authorization.
- **Intentional:** machine-only, immutable, or safety-owned behavior records why it is not an
  operator control.

The inventory lives in
`runinator-command-center/src/core/navigation/control-surface-coverage.json`. Command Center exposes
it under **Administration → Capability Coverage**. A requirement outcome is `surface`,
`intentional`, or `gap`; every outcome requires a human-readable explanation. A visible gap may be
recorded while work is underway, but Phase 5 ships with none.

The web service adds `x-runinator-control-surface` to every OpenAPI operation. Operator operations
also receive `x-runinator-capability`; non-operator operations receive
`x-runinator-control-surface-reason`. The OpenAPI policy test compares operator capabilities with
the Command Center inventory, so a new operator endpoint cannot land uncategorized. Command Center
tests separately ensure that every recorded page is a real navigation target and that all five
requirements are answered.

When adding a capability:

1. Classify its paths in `runinator-ws/src/openapi/control_surface.rs`.
2. Add or update its inventory entry, using a rich typed page as the primary surface.
3. Use `intentional` only for a fixed protocol, machine role, deployment concern, or immutable
   evidence boundary and state the reason.
4. Use `gap` when the surface is genuinely missing; do not disguise unfinished operator work as an
   intentional boundary.
5. Run the web-service OpenAPI tests and Command Center tests.

Runtime secret resolution, scheduler claims, trigger scans, idempotency mutation, replica
heartbeats, broker relay, workspace transfer, and similar service protocols are headless by design.
Their endpoint policy must name the matching `SystemRole`; authenticated-only metadata would
incorrectly classify them as operator capabilities.
