# Repository layout and scaffolding

Status: scaffold implemented, September 2026; M0 byte-codec work has begun
in `core/`. Scaffold validation is recorded in
[002](../tactical/002-repository-scaffold.md).
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

The five Rust targets and root build files now exist. The core now contains
a canonical CBOR codec, operation decoder, crypto/HPKE primitives, signed
batch bytes, in-memory projection, HLC, and vector tests. `core-wasm/` now
calls the shared batch/projection path from JavaScript against fixed vectors.
`core-ffi/` exposes that path through UniFFI and passes selected Swift and
Kotlin encrypted vectors; `server/` and `cli/` remain scaffold boundaries.
The documentation, test, and platform paths marked for later milestones are future paths, not
links to existing files. Scaffold targets compile without implementing their
responsibilities. The CBOR vector tests cover only the first byte subset;
other targets reporting zero cases are not evidence of protocol or product
correctness. A separate real Chromium harness tests wasm with IndexedDB
reload, rollback, and Family key separation using fixture data. UniFFI's
MPL-2.0 runtime/build crates have exact-version
exceptions in `deny.toml`; generated native bindings are build outputs, not
committed project sources.

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
  recovery logic. Do not make it depend on the entire client core for
  convenience. Extract a narrowly shared envelope crate only if a concrete
  contract demonstrates the need; do not create one speculatively.
- Keep component unit tests beside their component. Shared scenarios and
  vectors use stable IDs to connect decisions to executable tests.

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
