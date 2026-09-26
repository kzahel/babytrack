# Protocol contracts

M-1 defines version 1 before application code depends on it. These files own
exact encodings and state transitions; the owning topics retain rationale and
product decisions. The version is not deployed and may still change during
the recorded security review. A change to a settled contract updates its
vectors and relevant scenarios in the same change.

| Contract | Scope |
|---|---|
| [Records v1](records-v1.md) | Canonical bytes, identities, operation and projection rules, time, unknown data |
| [Sharing v1](sharing-v1.md) | Control chain, batches, invitation, grants, removal, promotion, relay behavior |
| [Portable file v1](portable-file-v1.md) | State backup, optional protection, atomic restore |

Symbolic user outcomes live in `docs/scenarios/`. Exact inputs and expected
outputs live in `tests/vectors/`; M0 binds them to Rust, Swift, Kotlin, and
browser runners. No executable implementation exists yet.
