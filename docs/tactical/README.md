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
  pinned developer-preview invitation handoff and child-first local setup
  implemented; owns the
  two-physical-phone gate and remaining daily-use UI, import, accessibility,
  and recovery work.
- [006: M2 responsive web client](006-m2-web.md) — local preview and
  native-managed first, later, and rotated-epoch joins implemented; their
  bounded web trust review passed. Web-origin sharing remains open; verified
  removal, private copy, and file recovery pass browser UI flows.
- [007: Android screen extraction and fixture gallery](007-android-screen-gallery.md)
  — implementation and local validation complete; explicit Android screen
  boundaries, previews, and an organized offline fixture catalog are delivered.
  Render artifacts were verified in CI; local catalog checks and the hosted
  first-install join correction are recorded in 007.
- [008: Shared model and implementation cleanup](008-maintenance-refactors.md)
  — complete with local validation; shared event builders/read models,
  Android coordination, native bindings, relay storage, harness/documentation
  cleanup, and minimum-API
  compatibility, with a validated commit per slice.
- [009: Action and storage boundaries](009-action-and-storage-boundaries.md)
  — complete with local verification; web async state, Android action
  dispatch, harness diagnostics, wasm modules, portable core behavior, and
  client SQLite organization, with a validated commit per slice.
- [010: Android design and daily-use UX pass](010-android-design-pass.md)
  — complete with local verification; activity identity, Today tiles, a
  shared capture template, live nursing/pumping timers, History day view,
  and a settings-style Family screen. Physical-phone gates stay with 005.
- [011: Android local timer notifications](011-android-timer-notifications.md)
  — complete with local verification; quiet nursing/pumping notifications,
  session-bound navigation, dismissal, permission, and offline lifecycle
  recovery. Phone gates stay with 005.
- [012: Internal release preparation](012-internal-release-preparation.md)
  — active, owner-authorized early store draft setup; Android release bundle
  packaging, store records, signing, and upload prerequisites. No public
  release or milestone completion is implied.
