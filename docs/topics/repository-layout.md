# Repository layout and scaffolding

Status: bounded M0 foundation complete; the Android developer app and M2
web preview are implemented. [003](../tactical/003-m0-foundation.md) owns the
foundation gate, [005](../tactical/005-m1-android.md) and
[006](../tactical/006-m2-web.md) own product delivery, and
[008](../tactical/008-maintenance-refactors.md) records maintenance validation.
Owns directory placement, dependency direction, and when scaffolding is
introduced. The [MVP plan](../mvp-plan.md) owns stack and milestone scope.

## Current documentation

| Path | Role | Read when |
|---|---|---|
| `README.md`, `AGENTS.md` | Brief entry point and mandatory constraints | Starting work |
| `docs/mvp-plan.md` | Scope, stack, milestones, and review gates | Planning or cross-cutting changes |
| `docs/topics/` | Decisions, alternatives, limitations, open questions | The topic index routes the task here |
| `docs/tactical/` | Ordered delivery slices and completion tracking | Executing that workstream |
| `docs/scenarios/` | Symbolic acceptance cases, indexed by concern | Specifying or testing relevant behavior |
| `docs/product-proposal.md` | Preserved business/product background | Strategy and rationale, not implementation rules |

The former root `spec/` held only symbolic cases. Those now live in
`docs/scenarios/`, reflecting their current purpose. IDs and expectations
survive the move. Do not create empty directories to make a future tree
appear implemented.

## Implementation layout

| Path | Responsibility | Introduced |
|---|---|---|
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | Workspace, reproducible dependency/toolchain choices | Scaffolding |
| `wire/` | Canonical CBOR and domain-separated wire primitives shared without client storage or event logic | M0 relay |
| `core/` | Model, operations, projection, storage, crypto, sync, import/export | Scaffold target; behavior in M0 |
| `core-ffi/` | UniFFI boundary over the core for Swift/Kotlin | Scaffold target; binding proof in M0 |
| `core-wasm/` | wasm-bindgen boundary over the core for browsers | Scaffold target; binding proof in M0 |
| `server/` | Encrypted relay, authorization metadata, push interfaces | Scaffold target; behavior in M0 |
| `cli/` | Developer client invoking the shared core | Scaffold target; flows in M0 |
| `docs/protocol/` | Versioned wire/API/file contracts | M-1, when concrete contracts are written |
| `tests/vectors/` | Exact shared inputs/outputs consumed by language suites | Concrete format decisions; executed in M0 |
| `tests/` | Shared integration/fault harnesses | M0, as consumers are built |
| `apps/android/` | Phone/Wear modules and vendor implementations | M1 phone, M4 watch |
| `apps/web/` | Svelte product UI | M2; M0 browser proof belongs to test infrastructure |
| `apps/ios/` | iOS/watchOS apps and extensions | M3 phone, M4 watch |

The six Rust targets implement the bounded M0 foundation. `wire/` contains
canonical CBOR, public authority verification, and domain-separated
cryptographic primitives without client storage or plaintext event semantics.
`core/` owns operations, projection, local SQLite journals, shared history,
durable outboxes, membership, crypto, enrollment, sync, and portable files.
Portable event constructors and record read models are shared by native and
browser adapters; only native repository/storage coordination requires SQLite.

`core-ffi/src/` separates boundary records, local APIs, invitation reads,
shared authority/enrollment/actions/sync, and feature-gated fixtures. Its
Kotlin and Swift bindings are generated build outputs, not committed sources.
UniFFI's pinned MPL-2.0 build/runtime exceptions remain in `deny.toml`.
`core-wasm/` provides wasm APIs and the IndexedDB local/public/invitation
adapters. Browser journal and outbox transactions call Rust validation and
projection and cover concurrent tabs, reload, rollback, and removal copying.

`server/src/store/` separates schema initialization, opaque object staging,
authority/batch transactions, authenticated reads, integrity/history helpers,
and storage regressions. The relay uses `wire/`, never the client core.
The CLI and integration harnesses exercise encrypted native/browser exchange,
later enrollment, role changes, key rotation, and removal recovery.

`apps/android/` is the source-linked phone developer app for local tracking,
encrypted sharing, correction, timers, and readable/protected file recovery.
`apps/web/` is the Svelte product preview for local tracking, native-managed
joining, shared offline edits, verified removal/private copies, and readable
file restore. Web-origin sharing, physical-phone gates, iOS, and watches
remain with their milestone owners. Tests and vector inventories describe
specific executed coverage; a build alone is not protocol evidence.

The workspace uses package names such as `babytrack-core` rather than a Rust
crate named `core`, and pins Rust 1.92.0. Path names above remain short.

## Dependency boundaries

- `core-ffi`, `core-wasm`, and `cli` consume the same core. Bindings translate
  values/errors and manage lifetimes; they do not duplicate behavior.
- Platform UI consumes bindings and app-owned service interfaces. Vendor
  libraries stay inside their implementation modules.
- The core owns storage semantics; platform adapters supply capabilities.
  Validate SQLite/native and IndexedDB/browser transaction behavior before
  freezing the storage API.
- The relay must build without plaintext event semantics or client key
  recovery logic. It may use `wire/` for exact CBOR and public cryptographic
  primitives, but cannot depend on the entire client core. Relay authority
  validation remains server-owned and must be cross-checked with client
  vectors and scenario tests.
- Keep component unit tests beside their component. Shared scenarios and
  vectors use stable IDs to connect decisions to executable tests.

## Android presentation and render infrastructure

Android keeps production presentation beside the route and focused sharing
and backup controllers in
`apps/android/app/src/main/java/org/babytrack/app/`. Screens take display
state and callbacks; their fixtures live only in `src/debug/`, and the
Roborazzi/Robolectric renderer lives in `src/test/`. The shared Rust bindings
still supply the data model and actions. No fixture renderer initializes
native stores or calls a relay.

`scripts/render_android_gallery.sh` generates PNGs, metadata, and an offline
HTML gallery under the gitignored Android build directory. The Android CI
job retains that directory as a synthetic-data artifact. The local commands
and file map are in the [Android README](../../apps/android/README.md), and
work status is in [007](../tactical/007-android-screen-gallery.md).

The Android command-driven UI helpers live in `scripts/android_ui.py`.
Caregiver smoke, recovery, and navigation capture entry points import that
support module; importing helpers never runs a smoke flow.

## Validation and alternatives

The scaffold proves a clean checkout can build the selected targets and run
documented checks. M0 separately proves real Swift/Kotlin/wasm calls,
storage, crypto, and relay behavior. Follow 002 and the root README for the
current commands and results.

Creating all platform projects and placeholders now was considered and
deferred: it would introduce manifests, toolchains, and maintenance before
their consumers exist. A permanent top-level `spec/` was also considered;
keeping prose contracts under documentation and executable fixtures under
tests gives their maturity and purpose a clearer home.

## Reconsider if

- Cross-language tools require a different shared fixture location.
- Actual build boundaries demonstrate a need to split the core.
- A directory's responsibilities become independently testable concerns;
  update this map and its consumers together instead of adding parallel maps.
