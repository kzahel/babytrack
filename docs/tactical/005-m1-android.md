# 005: M1 Android caregiver app

Status: in progress on emulators alongside [003 M0 foundation](003-m0-foundation.md).
The [MVP plan](../mvp-plan.md#milestones) owns scope and milestone order;
the [event model](../topics/event-model.md) and
[Family sharing contract](../topics/family-sharing-and-trust.md) own product
semantics. This tactical owns Android delivery evidence and remaining work.

## Goal and exclusions

Deliver an accountless Android developer build that a caregiver can use on
their own phone for local tracking, file recovery, and encrypted sharing with
another caregiver. Android owns presentation, relay transport, Keystore
wrapping, scheduled work, and file picking. Rust owns record meaning,
storage, authorization, and sync. No app-store, hosting, clinical advice,
or production Family data precedes the M0 exit gate.

## Current evidence and limits

The debug app has local Family and child creation, Family switching,
timeline, bottle, diaper, sleep timer/completed sleep, note, whole-unit
growth, Celsius, medication, solids, completed multi-segment breast feeds,
and pumping logs. A confirmed timeline action tombstones one activity after
the shared core checks its Family and child. Readable and password-protected full files restore into a
fresh local Family. A command-driven headed
emulator check creates a Family and child, logs and deletes a diaper, records
an alternating breast feed, and verifies both states after restart. The
timeline can correct a note's text through the shared Rust edit path while
retaining its Family, child, activity ID, original time, and other fields.
It can also correct a bottle's whole-millilitre amount on the same event;
the content code and original time remain intact.
The Android surface now follows the device's light or dark mode for both
Compose content and system bars; emulator captures checked each mode.
Child creation can also record a birth date and the sex code needed for
eventual growth charts. The shared core persists these as child metadata in
local and shared history; growth percentiles are still pending.

Two separate emulator installations and a disposable relay pass first-cohort
join without simultaneous foreground use, reciprocal encrypted edits,
recipient timer stopped by the manager, growth and temperature delivery,
medication, solids, a completed alternating breast feed, and pumping delivered to the
manager, a manager deletion converging to the recipient, offline pending work, manager removal,
recipient proof, and automatic private copy. Foreground polling runs every
30 seconds; a scheduled job requests sync when Android allows it. Push
remains a later latency option.
The disposable two-emulator flow now restarts the relay after the manager's
rotating removal, then checks the recipient's removed state and private copy
against the reconstructed authority log.
The early independent authority recheck passed for that implemented cohort
at `82c0f4c`; [003](003-m0-foundation.md#early-m0-authority-recheck-at-82c0f4c)
records its limits. No physical two-phone or full M0 exit gate has passed.
Sharing setup and join controls are still debug UI with manual relay origin,
public key, and optional invitation-fragment entry. Android's share sheet can
send a one-use fragment, and receiving a text fragment prefills the join form
without claiming it. A focused emulator instrumentation test covers that
handoff. This is still a development flow, not a released product flow.

## Ordered slices and gates

### 1. Local caregiver flow on one phone

- [x] Create and switch local Families, add/select children, log bottle and
  diaper, and inspect a persisted timeline through the shared Rust core on
  an emulator. The UI smoke runs after restart in CI.
- [x] Log sleep timers/completed sleep, notes, growth in whole grams and
  millimetres, and entered Celsius decimals through the local/shared core.
- [ ] Complete the MVP logging set: growth percentile display and practical
  edits beyond notes and bottle amount. Multi-segment breast feeds, note and
  bottle correction, and
  single-activity deletion now pass
  local restart/file restore, two-emulator encrypted sync, and UI tap flows.
  Preserve unknown fields and exact Family/child targets. Keep clinical
  recommendations out of the UI.
- [ ] Run local logging, Family switching, and restart on a physical Android
  phone. Check accessible labels, large text, dark night use, startup size
  and latency, and the common feed/diaper path with one hand.

Gate: the physical-phone flow works offline without an account, saves to the
intended Family and child, and remains usable after process death.

### 2. Two-caregiver sharing and background progress

- [x] Exercise first manager and one recipient on separate emulators through
  invite, claim, challenge, proof, grant, encrypted sync, delayed foreground
  passes, scheduled work, and first recipient removal/private copy.
- [ ] Complete general device membership, manager/role changes, invite
  cancellation, and key rotation in [003](003-m0-foundation.md) before
  exposing those actions as ordinary Android sharing controls.
- [ ] Make invitation handoff usable through a share/link flow with trusted
  relay pinning and clear pending, accepted, blocked, and removed states.
  The share-sheet receive path now prefills without auto-claiming. Remove
  debug-only gating after the M0 exit security review passes.
- [ ] Run the [two-caregiver gate](../mvp-plan.md#testing-and-validation)
  on two physical phones: delayed join, offline edits on both, convergence,
  removal with pending work, private-copy destination, and failed background
  wake/retry. Verify a suspended app does not claim immediate progress.

Gate: two phones converge on verified history without simultaneous app use;
the UI never labels a keyless pending join ready or an uncertain upload
delivered, and removed access cannot obtain new Family data.

### 3. File recovery, import, and daily use

- [x] Save readable or password-protected full files and restore them into
  an independent local Family with a visible saved point. The core checks
  corruption and wrong passwords without partial Family creation.
- [ ] Check Android file save/restore on physical phones and across a new
  installation. Test missing/corrupt files, storage-provider failures, and
  explicit presentation of data saved after the file point.
- [ ] Add the planned analysis export and Nara import through shared Rust
  import/export, with real sanitized samples and target preview. The
  current-state analysis CSV now exports local and shared Families through
  the core, including pending local shared edits; Nara import remains. No
  sanitized Nara export is available yet (confirmed 2026-09-27), so its exact
  column mapping and sample-backed validation remain open.
- [ ] Add an ongoing notification for timers and basic widgets without
  reimplementing event semantics in Android. Confirm stale actions after
  removal target a private copy or fail visibly.

Gate: caregivers can move a saved point to another phone and understand
what it contains; daily logging and recovery require no relay account.

## CI and review handoff

The Android CI job builds APKs. Its emulator job runs the real-relay
instrumentation suite and the command-driven local UI smoke. Expand UI
checks for sharing and backup only where a failure would escape the core
integration suite. Record local emulator, physical phone, and observed
remote CI results separately. The M0 end review in [003](003-m0-foundation.md)
must pass before using real Family data. At the two-phone gate, report
observable flow status and unresolved risks to the user; routine code and
API decisions do not require a separate approval step.

Completion: the physical one-phone and two-caregiver gates pass, the planned
MVP tracking and recovery set is usable in Android developer builds, and
the relevant independent security gate and CI checks have recorded results.
