# Tactical workstreams

Tactical documents are executable workstream plans. This index lists every
tactical with its current status.

## Workstreams

- [001: M-1 design closure](001-pre-m0-design.md) — complete; product scope,
  Family authority, data model, recovery, versioned protocol and vectors,
  and independent preimplementation security review settled.
- [002: Repository scaffolding](002-repository-scaffold.md) — complete;
  Rust workspace, build checks, and read-only CI added and validated in an
  isolated source copy. Does not close M-1 or implement product behavior.
- [003: M0 executable foundation](003-m0-foundation.md) — in progress;
  Rust canonical CBOR, operation, crypto/HPKE, batch bytes, and local-only
  projection cases run; full binding, storage, relay, membership, and
  recovery gates remain.
- [004: Autonomous delivery tracker](004-delivery-tracker.md) — active
  coordination; tracks the next M0 proof, security-review handoffs, CI
  growth, and progress toward a usable M1 Android UI. Detailed completion
  status stays with each implementation tactical.
