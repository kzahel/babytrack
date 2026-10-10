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

1. [x] One web action boundary. Add a JSON action entry point in
   `core/src/web_actions.rs` that builds every Android create and correction
   operation through the portable `event_actions`, looking up correction
   targets in the projection. Expose it on the local and shared wasm
   families, return the full read-model activity fields in the snapshot,
   and expose the core day summary. Byte-parity tests compare browser and
   native operations for each action.
   Done: `web_actions::action` parses 29 create/correction intents and builds
   them through `event_actions`; `core/tests/web_action_parity.rs` checks
   each against the native `local_api` bytes and rejects unknown fields,
   bad IDs, negative starts, and wrong targets. Local and shared wasm
   families expose `action_operation`, `day_summary_json`, and
   `analysis_csv`, which is now portable. Every existing web write uses
   `act`; the local and real-relay browser flows pass unchanged.
2. [x] Capture parity: bottle units and repeat amount, solids, pumping with a
   local timer and amounts, growth with entered units, temperature with
   unit, medication, completed past sleep and sleep place, and a chosen
   activity time where Android offers one. Edit a child's name, birthday,
   and growth sex.
   Done: an Add activity chooser in Android's groups and order opens one
   form per type with Android's codes, defaults, and validation; each form
   except the breast timer has a When row with a past date and time. Bottle
   has units, steppers, and Same as last; breast has Timer and Enter
   minutes; pumping has a browser-local stopwatch that fills its minutes;
   sleep saves a past interval or starts a timer, with place. Child profiles
   create and edit name, birthday, and growth sex; code 3 now reads
   Unspecified as published. History lines follow Android's summaries.
   The browser smoke saves every type through the chooser, checks its
   summary, edits a child, and the real-relay flow uses the chooser.
3. [x] Correction parity: per-type field edits, instant time edits, moving a
   completed interval, sleep end and place edits, breast start correction,
   and delete with Undo.
   Done: tapping a History or recent row reveals Android's actions for its
   type — add or edit a note on any entry, edit time on instant entries,
   move a completed sleep or pump, sleep duration and place, bottle, diaper,
   solids, pumping, medication, growth (blank keeps a value), temperature,
   and breast feeds with a start correction and Keep finish or Keep start —
   plus confirmed delete with a timed Undo that restores the same entry.
   The browser smoke makes each correction, checks a future-ending breast
   edit is refused, and deletes and restores an entry.
4. [x] Today and History parity: activity state tiles with last events and
   the running sleep, the core day summary, History day view with a week
   strip and date choice, day totals, All mode, and type filters.
   Done: Today shows Android's feeding (or nursing timer), sleep (running
   clock and Stop, or last sleep with Start and Add past sleep), and diaper
   (Wet now, Log diaper) tiles, Add activity, So far today lines from the
   core day summary, and three recent entries that open History. History
   has Day and All days, a seven-day strip with a date picker, Android's
   filters, core day totals for the chosen day, intervals on every day they
   touch, and day headings in All mode. The smoke checks the tiles, summary
   lines, a filter, and an empty earlier day.
5. [x] Data parity: password-protected backup save and restore through the
   shared Argon2id file contract, and the analysis CSV export.
   Done: the core's protect/open functions and the analysis CSV now build
   for wasm (their pure-Rust dependencies moved out of the native-only
   table; `Cargo.lock` is unchanged). Data and backups saves readable or
   password-protected files, exports the CSV, shows the last local save and
   unsaved changes, and restores through a preview of the file's save time,
   record count, and known gap. The browser smoke round-trips a protected
   file into a fresh profile, rejects a wrong password without creating a
   Family, and checks the CSV; the shared flow restores via the preview.
   Switching Family no longer resets a tab chosen while the Family loads.
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
