# 009: Action and storage boundaries

Status: complete with local verification, 2026-09-30. All six requested
follow-up refactors are delivered, with a commit for each validated slice.

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
3. [x] Bound Android harness subprocesses, emit stage progress and failure
   artifacts, and split browser smoke into individually runnable scenarios.
4. [x] Separate wasm bindings by local/enrollment/authority/restore/fixture
   concern while preserving generated JavaScript exports and feature gates.
5. [x] Move remaining pure event construction and day summaries out of the
   native-only API, retaining ID/clock adapters and compatibility reexports.
6. [x] Separate SQLite schema, journal, outbox/history, enrollment, and copy
   persistence into modules while preserving whole transaction ownership.
7. [x] Run final affected cross-platform checks and reconcile documentation.

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

- Slice 3: Android subprocesses now have bounded timeouts; scenario wrappers
  emit progress and retain available screenshot/logcat/XML failure evidence.
  Three regressions cover actual subprocess timeout, diagnostic failure that
  preserves the original exception, and partial artifact capture. A controlled
  failure on a disposable emulator confirms real capture. Browser smoke now
  has nine named, independently isolated scenarios; all pass and a filtered
  rebase run passes. A source comparison confirms unchanged scenario bodies,
  assertions, and fixture inputs apart from relay routing. A controlled native
  exchange failure retains the original assertion, relay log, and Playwright
  trace and cleans up the relay/browser. CI retains diagnostics when present.
  Python/JS compilation, CLI help/list, shell syntax, Actionlint, boundaries,
  and diff checks pass. Complete UI/recovery/capture results are recorded at
  final verification; full UI/recovery/capture runs pass with the new support
  module.

- Slice 4: wasm boundary implementations now live in seven concern modules;
  the root retains public reexports and shared boundary helpers. Generated
  TypeScript class/function signatures compare equal for both fixture and
  production builds. Wasm check/Clippy, fixed encrypted-vector smoke,
  production fixture-exclusion checks, all nine browser scenarios, and local
  and shared web UI flows pass. Workspace boundaries and diff checks pass.
  No protocol interpretation or generated export changed.

- Slice 5: 34 remaining public event constructors/corrections and their pure
  helpers move into portable child, interval, measurement, correction, and
  tracking modules. Input types and day summaries are portable, with existing
  native paths retained as reexports/error adapters. All 43 native builder
  signatures compare equal. The full core suite, canonical native/portable
  byte comparisons, and direct portable partial-edit/unknown-field/pump/day
  regressions pass. Core wasm check, Clippy, Swift/Kotlin and wasm fixed-vector
  smoke, production fixture-exclusion, boundaries, and diff checks pass.
  Native adapters still allocate IDs; SQLite still assigns persisted clocks.
  Android integration on the final storage build belongs to slice 7.

- Slice 6: the client SQLite facade owns its connection and public types;
  eight modules own schema/migrations, Families, copies, journal, outbox,
  verified history, enrollment, and prepared authority persistence. All 48
  method signatures/bodies compare unchanged apart from formatting; no
  transaction is split or connection added. Original unit-test module paths
  remain stable. The full core suite (including late-failure copy/enrollment
  rollback, concurrent migrations, reopen/exact-byte outbox retry, and pending
  write tests), workspace Clippy, boundaries, and diff checks pass. Final
  Android unit/lint/APK builds also pass against the extracted storage.
  Cross-platform integration and final status reconciliation follow in slice 7.

- Integration follow-up: the first final 28-case relay run exposed a transient
  SQLite-busy race between the recreated activity's join coordinator and the
  test's independent assertion connection. The assertion now retries only
  `DatabaseBusy` within its existing deadline, matching the neighboring join
  test; all other errors still fail immediately. The focused recreation case,
  clean-install invitation case, and complete 28-case relay rerun pass.
  Production transactions and coordinator behavior are unchanged.

## Final local verification

Final product code is at `6b4800f`; the bounded join assertion retry is at
`e17067a`. The six implementation slices and the integration follow-up are
committed individually. The commands below exercise those product inputs.

| Check | Observed result |
|---|---|
| Rust formatting, workspace check/test/Clippy, wasm target check | Pass; 132 workspace tests, including portable actions and SQLite migration/rollback/retry regressions |
| Cargo deny advisories/bans/licenses/sources | Pass |
| Swift/Kotlin and wasm fixed-vector smoke; production fixture boundary | Pass |
| Generated wasm API comparison | Production and fixture TypeScript class/function signatures unchanged |
| Browser wasm/IndexedDB/relay | All nine isolated scenarios pass; filtered scenario and controlled failure evidence recorded in slice 3 |
| Responsive local and real-relay shared web UI | Pass, including reload, offline edits, rotated joins, removal copies, and backup restore |
| Android unit/lint/debug and test APK builds | Pass |
| Android fixture gallery | 31 cases, 124 variants, 214 images; representative large-font light and dark captures inspected |
| Android clean-install invitation and real relay | One independent invitation case and all 28 suite cases pass after the bounded assertion retry; focused recreation case also passes |
| Full caregiver walkthrough | Pass on the final APK, including Family/child selection, logging, correction, deletion, and restart |
| Document-picker recovery | Readable/protected backups, damaged-file denial, fresh-install restore, and restart pass |
| Two-emulator relay flow | Pass: independent installations join, exchange encrypted edits, resume after relay restart, recover a removal copy, and join again at the later epoch |
| Navigation capture | Pass after action/harness changes; light/default and dark/large-font routes captured |
| Harness/automation/repository checks | Three Python regressions, Python/JS compilation, help/list/filter, shell syntax, Actionlint, workspace boundaries, and diff checks pass |

These are measured local runs. No new remote CI run is claimed. Physical-phone,
web-origin authority, scale, and release gates remain with their existing
milestone owners. Both owned read-only emulators were shut down and reaped;
no owned browser or relay processes remain. The workstream is complete.
