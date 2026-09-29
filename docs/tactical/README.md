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
- [003: M0 executable foundation](003-m0-foundation.md) — bounded exit
  complete; shared-core bindings, durable native/browser storage, encrypted
  relay, joining, removal/private copy, recovery, and mixed-client exchange
  passed the required CI suite and independent security gate. Extended
  coverage and scale work is tracked in its post-M0 queue.
- [004: Autonomous delivery tracker](004-delivery-tracker.md) — active
  coordination; tracks the next M0 proof, security-review handoffs, CI
  growth, and progress toward a usable M1 Android UI. Detailed completion
  status stays with each implementation tactical.
- [005: M1 Android caregiver app](005-m1-android.md) — emulator flows and
  pinned developer-preview invitation handoff implemented; owns the
  two-physical-phone gate and remaining daily-use UI, import, accessibility,
  and recovery work.
- [006: M2 responsive web client](006-m2-web.md) — local preview and
  native-managed first, later, and rotated-epoch joins implemented; their
  bounded web trust review passed. Web-origin sharing remains open; verified
  removal, private copy, and file recovery pass browser UI flows.
