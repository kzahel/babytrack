# 002: Repository scaffolding

Status: planned. Layout/documentation preparation is complete; executable
scaffolding has not started. The [layout topic](../topics/repository-layout.md)
owns paths and dependency boundaries. [001](001-pre-m0-design.md) remains
the gate for product/protocol implementation. The current scope is a
directory/build plan, not authorization to create runnable targets now.

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

- [ ] Inspect available Rust/toolchain support and pin a compatible version.
  Record prerequisites without requiring credentials or a running service.
- [ ] Add root workspace configuration, lockfile, and minimal build targets
  for `core`, `core-ffi`, `core-wasm`, `server`, and `cli` as documented.
  Keep crate APIs empty until there is a concrete consumer; do not invent
  example Family structs, protocols, or no-op security functions.
- [ ] Verify dependency direction. Core-facing targets depend on the core;
  the relay does not pull in plaintext client semantics.
- [ ] Establish native build and a wasm target compile check. Defer generated
  Swift/Kotlin bindings and their execution proof to the first M0 slice.

### 3. Checks and development instructions

- [ ] Add local fmt, clippy, test/build, and license/dependency checks that
  actually run; record exact commands in `AGENTS.md` and the relevant README.
  Identify zero-test targets explicitly.
- [ ] Add bounded read-only CI with an always-running aggregate required
  check. Avoid release credentials and publishing jobs during preparation.
- [ ] Activate dependency-update entries only for manifests/workflows that
  now exist; add other ecosystems with their actual projects.
- [ ] Verify the documented sequence in a clean checkout or equivalent
  isolated build directory, retaining the exact commands and results.

### 4. Handoff

- [ ] Update status and index when the scaffold is real and verified.
- [ ] Identify the next M0 portability proof and its 001 prerequisite;
  preserve the requirement for real Swift, Kotlin, and browser execution.

## Gates and completion

002 completes only when targets and checks exist, the documented build
works, dependency boundaries hold, and the workspace makes no unsupported
claim about behavior. Update this tactical and the index in that change.
Scaffolding completion does not close 001 or authorize protocol assumptions.
Unsettled product/security decisions remain explicit blockers for the
implementation slices that depend on them.
