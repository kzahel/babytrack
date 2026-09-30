# 008: Shared model and implementation cleanup

Status: active, 2026-09-30. The user requested all six maintenance items
from the source review, delivered autonomously with a commit per slice.
[005](005-m1-android.md) and [006](006-m2-web.md) retain product delivery
gates; this workstream owns refactor progress and validation evidence.

## Goal and exclusions

Reduce duplicated event behavior and separate implementation responsibilities
without changing the v1 protocol, exported client APIs, Family promises,
navigation, draft retention, or transaction guarantees. Keep Rust as the
owner of event semantics and keep the relay independent of plaintext data.
Resolve the recorded Android minimum-API lint failures rather than hiding
them behind a lint baseline. No new features, new crates, visual redesign,
hosting, publishing, or physical-phone gate claims are included.

Owning decisions: [event model](../topics/event-model.md),
[sync](../topics/sync-and-encryption.md),
[Android navigation](../topics/android-navigation.md),
[web client](../topics/web-client.md), and
[repository layout](../topics/repository-layout.md).

## Ordered delivery slices

1. [x] Unify native and browser event builders and overlapping read models
   in platform-independent core modules. Retain platform ID/clock/storage
   adapters and browser JSON shape. Add parity checks for overlapping types,
   invalid input, target identity, and edited breast-feed records.
2. [x] Extract Android sharing/enrollment and file backup/recovery
   coordination from TrackerRoute, and consolidate its loaded screen state.
   Keep platform launchers at the route boundary, the existing shared save
   coroutine lifetime, exact selection targets, and failed-save drafts.
3. [x] Organize native binding records/conversions, local APIs, sharing,
   invitations, and fixture APIs into modules. Move reusable native sync
   orchestration into core while preserving UniFFI exports and API signatures.
4. [x] Separate relay storage tests and implementation concerns:
   initialization/integrity, object staging, authority commits, and reads.
   Preserve explicit SQLite transaction ownership and wire validation.
5. [x] Audit obsolete scaffolding comments and broad dead-code allowances;
   refresh the current repository map; move Android UI harness helpers into
   a dedicated support module with all entry points updated.
6. [x] Resolve Android API 26 compatibility for generated binding cleanup
   and navigation-bar resources. Run lint without a NewApi suppression or
   baseline and add that check to the Android CI gate.
7. [ ] Run the affected cross-platform integration and boundary checks,
   reconcile owning documentation, and record the final completion evidence.

## Validation and completion

- Each implementation slice updates this document and commits its completed
  scope and measured evidence. Do not record an unrun check as passing.
- Shared model: core tests, workspace formatting/Clippy/tests, wasm build and
  byte/browser smokes, and native Kotlin/Swift smoke.
- Android: unit tests, both debug APKs, fixture gallery, lint for compatibility,
  clean-install invitation and existing real-relay/UI/recovery checks on a
  disposable emulator when available. Use bundled test tools, not a primary
  installed browser or personal phone.
- Bindings: native smoke and fixture API boundary; preserve production and
  fixture feature builds and generated Kotlin/Swift public names.
- Relay: server and CLI real-relay tests, workspace dependency boundary,
  existing authority/negative tests and transaction behavior.
- Harness: Python compilation, command entry points, actual UI/recovery
  checks, shell syntax for modified scripts, and diff checks.
- This is maintenance within reviewed trust boundaries, not a new MVP
  security gate. Any newly introduced authority or protocol behavior must
  return to its owning topic/scenarios and named independent review gate.

Completion requires every requested item delivered, relevant checks passed
or a concrete environmental limit recorded, and a clean committed tree.
M1 physical-phone, M2 web-origin sharing, and release gates remain with their
existing owners.

## Evidence

- Planning: source review found duplicate child/activity construction in
  native `local_api` and portable `web_actions`, duplicated native pull and
  object hydration orchestration, a large Android route controller, relay
  tests embedded in storage, and outdated scaffolding comments/layout text.
  [007](007-android-screen-gallery.md#evidence) records the existing five
  Android NewApi lint failures. No implementation checks have run yet.

- Slice 1: portable `event_actions` now constructs child, diaper, bottle,
  note, breast-feed, and breast-feed correction operations for both adapters.
  `read_model` owns typed record decoding; native reexports and browser JSON
  shapes remain stable. Creation now checks trimmed child-name byte length
  consistently. Two parity regressions compare canonical bytes, corrected
  identity, display fields, limits, and invalid input. Core tests, workspace
  Clippy, rustfmt, wasm byte smoke, Kotlin/Swift smoke, IndexedDB/real-relay
  browser smoke, local web UI, and shared web UI all pass locally. The two
  web UI runners rebuild the same directory and must run sequentially; a
  concurrent invocation collided during npm/build and passed when rerun alone.

- Slice 2: Android sharing/enrollment actions and foreground sync passes now
  live in `TrackerSharingController`; document launchers and file actions
  live in `TrackerBackupController`. Both use the route's existing scope.
  The route publishes one immutable `ScreenData` instead of assigning its
  loaded fields individually. Unit tests and both APKs pass; the clean-install
  invitation test and all 28 real-relay cases pass on a read-only disposable
  emulator. The quick create/diaper/edit/delete/restart UI flow and complete
  readable/protected file recovery pass at 1.5× text. Recovery exposed existing
  unscrolled capture/password lookups; those now scroll rather than assuming
  controls fit above the fold. The gallery renders 31 cases/124 variants;
  an actual large-text removed-Family render was inspected. Workspace
  boundaries, Python compilation, and diff checks pass.

- Slice 3: native bindings now separate records/conversions, local APIs,
  invitations, shared authority/enrollment/actions/sync, and fixtures. Root
  reexports retain public names; the entry point is 87 lines. Core
  `active_pull::pull_and_hydrate` owns the previously repeated bounded pass,
  retaining signing through core credentials and app-owned transport. Core
  tests, workspace Clippy/format, native Kotlin/Swift smoke, production
  fixture exclusion, Android unit tests and both APKs, the clean-install
  invitation case, and all 28 relay cases pass. All 514 generated Kotlin
  declaration signatures match the saved pre-refactor production snapshot.
  Workspace boundaries and diff checks pass.

- Slice 4: `server/src/store/` separates initialization, opaque staging,
  authority/batch commits, authenticated reads, integrity, object/history
  validation helpers, and tests. The storage facade is 117 lines and the
  former embedded regressions occupy `store/tests.rs`. Internal helpers have
  store-scoped visibility; complete transaction bodies remain in their
  owning commit methods. All 23 server tests and six CLI integration tests
  pass, including dynamic authority and encrypted relay exchange. Workspace
  Clippy, rustfmt, dependency boundaries, and diff checks pass.

- Slice 5: removed obsolete relay scaffolding comments and blanket item
  suppressions. Diagnostic error enums retain narrowly documented allowances;
  four raw storage helpers are now explicitly test/test-harness-only. Removed
  one unused challenge-context copy while retaining shared-verifier validation.
  Native-only bootstrap helpers are correctly excluded from wasm. Production
  relay, workspace, and wasm Clippy pass without warnings; server/CLI tests
  pass. The repository map and stale event/sync status introductions now link
  to current gate owners. All three Android script entry points import
  `android_ui.py`; the 14 moved helper ASTs are unchanged, Python compilation,
  CLI help, and import-only checks pass without starting a scenario. The full
  caregiver UI walkthrough is running; its final result, recovery, and capture
  verification will be recorded with slice 7. Diff/boundary checks pass.

- Slice 6: Android generation now uses a dedicated UniFFI global config,
  selecting SDK-guarded Android cleanup and JNA below API 34 without changing
  JVM smoke generation. An API 26 Robolectric test proves fallback selection
  and exactly-once explicit cleanup. The first run correctly failed with the
  old JVM backend; correcting the pinned generator's global config schema
  made it pass. API 27 navigation-bar attributes now live in version-qualified
  day/night resources. All six Android unit tests, both debug APKs, and
  `lintDebug` pass with no NewApi suppression or baseline. CI now runs lint;
  the generator script/configuration are tracked Gradle inputs.
