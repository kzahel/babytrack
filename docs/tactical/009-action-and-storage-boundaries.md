# 009: Action and storage boundaries

Status: active, 2026-09-30. The user requested all six follow-up refactors,
with autonomous implementation and a commit for each validated slice.

## Goal and exclusions

Separate platform coordination, bindings, and storage responsibilities while
preserving protocol bytes, generated exports, transaction guarantees, saved
targets, draft retention, and existing product flows. Add protection against
stale web async results and improve diagnostic harness reliability.
No product features, new crates, hosting, publishing, or milestone closure.

Owning topics: [web](../topics/web-client.md),
[Android navigation](../topics/android-navigation.md),
[event model](../topics/event-model.md),
[sync](../topics/sync-and-encryption.md), and
[repository layout](../topics/repository-layout.md).

## Ordered delivery slices

1. [x] Extract web selection/loading/join/poll coordination, publish coherent
   snapshots, reject stale async results, and regress delayed Family switching.
2. [x] Centralize Android local/shared action dispatch without moving event
   semantics into Kotlin or changing the route's coroutine/draft lifetime.
3. [ ] Bound Android harness subprocesses, emit stage progress and failure
   artifacts, and split browser smoke into individually runnable scenarios.
4. [ ] Separate wasm bindings by local/enrollment/authority/restore/fixture
   concern while preserving generated JavaScript exports and feature gates.
5. [ ] Move remaining pure event construction and day summaries out of the
   native-only API, retaining ID/clock adapters and compatibility reexports.
6. [ ] Separate SQLite schema, journal, outbox/history, enrollment, and copy
  persistence into modules while preserving whole transaction ownership.
7. [ ] Run final affected cross-platform checks and reconcile documentation.

## Gates and completion

Each slice records measured validation and commits its implementation and
status together. Use existing core, native/wasm, fixture-boundary, browser,
web UI, Android unit/lint/APK/gallery, real-relay/UI/recovery, and two-emulator
runners as relevant. Mechanical extractions retain public interfaces and
transaction bodies. Add focused regressions for behavior changes, not tests
that mirror mechanical moves. Use disposable test browsers/emulators and
reap all owned processes. These changes stay within existing trust boundaries;
new authority/protocol behavior would return to the named security gate.

Completion requires all requested work delivered, measured checks recorded,
and a clean committed tree. Physical-phone, web-origin authority, and release
gates remain with their milestone owners. Local runs do not imply remote CI.

## Evidence

- Planning: inspection found repeated Kotlin local/shared dispatch, mutable
  web selection used across awaits, unbounded ADB subprocesses, a monolithic
  browser smoke, interleaved wasm exports, remaining portable behavior in
  native `local_api`, and multiple SQLite responsibilities in one source file.

- Slice 1: the web controller owns selection/loading, joins, polling, and
  removal/restore coordination; rendering and drafts remain in the component.
  Selection generations, refresh ordering, and disposal guard publications.
  Save completion retains its target and private-copy child selection.
  Six focused async regressions pass, along with local and real-relay shared
  web UI flows (joins, rotation, removal, offline edits, export/restore/reload).
  The build runner includes the controller regressions. Boundary and diff
  checks pass; no protocol, authority, or IndexedDB contract changes.

- Slice 2: `TrackingActions` and its router replace 33 repeated dispatch
  branches across capture, correction, quick actions, child actions, and Undo.
  The native adapter and existing sharing coordinator implement the interface;
  saved target flags and IDs remain call arguments. A source comparison proves
  exact call/argument preservation across all 33 replacements. Android unit
  tests, lint, both APKs, 31-case gallery, clean-install invitation and all 28
  real-relay cases, and the quick caregiver edit/delete/restart UI flow pass.
  Coroutine ownership and draft clearing callbacks are unchanged. Diff checks
  pass; the full walkthrough is included in final verification.
