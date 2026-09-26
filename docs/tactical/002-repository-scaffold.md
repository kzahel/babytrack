# 002: Repository scaffolding

Status: complete. The [layout topic](../topics/repository-layout.md) owns
paths and dependency boundaries. [001](001-pre-m0-design.md) remains the
gate for product/protocol implementation.

## Goal

Give the repository a small, reproducible build foundation without choosing
unsettled data, crypto, storage, or access semantics. A developer should be
able to identify targets and run the documented checks from a clean checkout.

## Exclusions

- No event schemas, cryptographic constructions, membership state machine,
  storage schema, wire endpoints, or product UI.
- No generated empty platform projects or placeholders for every future file.
- No hosting, publishing, production credentials, or implementation of
  proposed UX decisions.
- No claiming a successful build or zero-test run satisfies behavioral gates.

## Ordered slices

### 1. Entry point and layout

- [x] Short README and task-directed agent reading route.
- [x] Background proposal separated from implementation requirements.
- [x] Symbolic cases moved to `docs/scenarios/` with an area index.
- [x] Current/future paths and dependency direction recorded in the layout
  topic; protocol prose and executable vectors have distinct future homes.

### 2. Minimal Rust workspace

- [x] Inspect available Rust/toolchain support and pin a compatible version.
  Record prerequisites without requiring credentials or a running service.
- [x] Add root workspace configuration, lockfile, and minimal build targets
  for `core`, `core-ffi`, `core-wasm`, `server`, and `cli` as documented.
  Keep crate APIs empty until there is a concrete consumer; do not invent
  example Family structs, protocols, or no-op security functions.
- [x] Verify dependency direction. Core-facing targets depend on the core;
  the relay does not pull in plaintext client semantics.
- [x] Establish native build and a wasm target compile check. Defer generated
  Swift/Kotlin bindings and their execution proof to the first M0 slice.

### 3. Checks and development instructions

- [x] Add local fmt, clippy, test/build, and license/dependency checks that
  actually run; record exact commands in `AGENTS.md` and the relevant README.
  Identify zero-test targets explicitly.
- [x] Add bounded read-only CI with an always-running aggregate required
  check. Avoid release credentials and publishing jobs during preparation.
- [x] Activate dependency-update entries only for manifests/workflows that
  now exist; add other ecosystems with their actual projects.
- [x] Verify the documented sequence in a clean checkout or equivalent
  isolated build directory, retaining the exact commands and results.

### 4. Handoff

- [x] Update status and index when the scaffold is real and verified.
- [x] Identify the next M0 portability proof and its 001 prerequisite;
  preserve the requirement for real Swift, Kotlin, and browser execution.

## Gates and completion

002 completes only when targets and checks exist, the documented build
works, dependency boundaries hold, and the workspace makes no unsupported
claim about behavior. Update this tactical and the index in that change.
Scaffolding completion does not close 001 or authorize protocol assumptions.
Unsettled product/security decisions remain explicit blockers for the
implementation slices that depend on them.

## Validation record

On 2026-09-26, the repository was copied to an isolated directory without
`.git/`, `target/`, or `references/`. The following commands all passed there
with the pinned Rust 1.92.0 toolchain, Python 3, cargo-deny 0.20.2, and
actionlint:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --locked
cargo test --workspace --locked
cargo check -p babytrack-core-wasm --target wasm32-unknown-unknown --locked
python3 scripts/check_workspace.py
cargo deny check advisories bans licenses sources
actionlint .github/workflows/scaffold.yml
```

Every crate reported zero unit and documentation tests. The Python check
verified that FFI, wasm, and CLI depend directly on the core while the server
has no transitive core dependency. The GitHub Actions workflow has a read-only
Rust job and an always-running aggregate `required` job; it has not yet run
on GitHub. Dependency updates cover Cargo and GitHub Actions only.

The next M0 portability proof is a concrete encode/encrypt/decrypt round trip
through Rust, Swift, Kotlin, and a real browser wasm harness. It requires 001
to settle the relevant protocol and data contracts, threat assumptions,
examples, and negative cases before those implementations depend on them.
