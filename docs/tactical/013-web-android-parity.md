# 013: Web parity with Android

Status: active, opened 2026-10-10. The owner chose to bring the web client to
Android feature parity before any visual redesign: the web is then the
faster surface for design iteration, and Android and later iOS follow its
settled design. This tactical owns that parity work; [006](006-m2-web.md)
keeps the M2 web gates and the [web client topic](../topics/web-client.md)
owns route and storage decisions.

## Goal and exclusions

A caregiver using only the browser can do everything the Android app does
for daily tracking, correction, review, recovery, and Family sharing. Rust
keeps owning event semantics, validation, day totals, file formats, sync,
and authority; the web adds forms, presentation, and browser storage glue.
Behavior is checked by browser tests, including a real relay where sharing
is involved.

Excluded: visual redesign (the next workstream), notifications, widgets and
other OS integrations, Nara import (absent on Android too), the physical
phone gates in [005](005-m1-android.md), hosted preview deployment, and
protocol or storage-schema changes. A missing core capability is added to
the shared core for both platforms, not reimplemented in JavaScript.

## Ordered delivery slices

1. [ ] One web action boundary. Add a JSON action entry point in
   `core/src/web_actions.rs` that builds every Android create and correction
   operation through the portable `event_actions`, looking up correction
   targets in the projection. Expose it on the local and shared wasm
   families, return the full read-model activity fields in the snapshot,
   and expose the core day summary. Byte-parity tests compare browser and
   native operations for each action.
2. [ ] Capture parity: bottle units and repeat amount, solids, pumping with a
   local timer and amounts, growth with entered units, temperature with
   unit, medication, completed past sleep and sleep place, and a chosen
   activity time where Android offers one. Edit a child's name, birthday,
   and growth sex.
3. [ ] Correction parity: per-type field edits, instant time edits, moving a
   completed interval, sleep end and place edits, breast start correction,
   and delete with Undo.
4. [ ] Today and History parity: activity state tiles with last events and
   the running sleep, the core day summary, History day view with a week
   strip and date choice, day totals, All mode, and type filters.
5. [ ] Data parity: password-protected backup save and restore through the
   shared Argon2id file contract, and the analysis CSV export.
6. [ ] Sharing parity: promote a local browser Family to the relay, issue
   member or manager invitations, list devices with roles, promote/demote,
   remove a device with key rotation, cancel unused invitations, and act as
   an admitted manager from a browser that joined. This adds a
   browser-origin authority path; run the independent review in the
   [security review runbook](../security-review-runbook.md) before real
   Family data relies on it.
7. [ ] Reconcile: full validation, owning topics, the 006 status, the
   comparison topic, and the tactical index.

## Gates and completion

Each slice lands as its own validated commit with browser test coverage of
its user flows, and updates this file in the same change. Slice 6 is not
complete until its review is recorded and blockers are fixed. The workstream
completes when every Android user flow outside the exclusions has a web
equivalent exercised by an automated browser test.
