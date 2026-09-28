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
the amount-only core path retains the content code and original time.
Bottle logging now exposes all four content choices, and the timeline shows
which was saved. An edit can change amount and content together on the same
activity. Bottle logging and correction also offer mL, US fl oz, and UK fl oz;
the timeline shows the entered decimal and unit while Rust stores the exact
rounded millilitres. Core restart/file restore, Android UI, and encrypted
relay tests cover those values.
Diaper type corrections likewise update the same activity, preserving its
original time and any other fields. Local restart/file restore and the
two-emulator encrypted relay flow cover the correction.
Solids foods and amount can now be corrected together on the same activity;
the shared core checks the target and retains its original time. Local file
restore and the two-emulator encrypted relay flow cover the correction.
The Android surface now follows the device's light or dark mode for both
Compose content and system bars; emulator captures checked each mode.
Temperature logging and correction now offer °C and °F through the Rust
conversion and preserve the entered decimal and unit in the timeline. The
local file round trip and two-emulator encrypted sync check Fahrenheit;
the Android UI smoke checks entry and correction across restart.
Child creation can also record a birth date and the sex code needed for
eventual growth charts. The shared core persists these as child metadata in
local and shared history; growth percentiles are still pending.
Growth entries can now include head circumference in millimetres alongside
weight and length, or alone. The core projects and exports that measurement;
local file restore and encrypted two-device sync check its correction. A
headed emulator UI run logged and corrected the measurement across restart,
and a separate run saved a head-only growth entry.
Caregivers can correct weight, length, or head circumference on an existing growth entry through
the shared Rust operation path. A blank measurement retains its previous
value; deleting the entry remains the way to remove a measurement. Local
restart/file restore and two-emulator encrypted convergence cover this edit.
Completed sleep duration can likewise be corrected on the same event without
moving its start time. Rust local restore and two-emulator encrypted sync
exercise the correction.
Running and completed sleep logs can now include the published optional
place. Android shows it in the timeline; the Rust core carries it through
file restore and analysis CSV, and the two-device relay case checks delivery
for both running and completed sleep.
The same saved sleep event can now change or clear its place without moving
its start or end. The local restore case checks clearing and correction; the
Android UI and two-device relay checks cover the corrected place.
That relay case exposed the foreground uploader's former sixteen-operation
pass limit: the last queued child remained local after one tap. The pass now
drains up to 64 single-operation batches, then leaves any larger backlog
visible for another foreground or scheduled pass. The two-emulator flow
checks the manager outbox is empty before the recipient reads that child.
Entered Celsius can be corrected on the existing temperature event without
changing its time or unknown fields; local restore and encrypted sync cover it.
The selected child's name can now be corrected through a field-level Rust
operation without changing its ID, birth date, sex, or existing activities.
Local restart/file restore and the two-emulator relay flow check the edit.
Pumping amounts can now be corrected on the existing activity, including a
switch between left/right amounts and a single total. The Rust operation
clears the old form, retains the original interval and activity ID, and is
checked across local restore and encrypted two-device sync.
Medication name, amount, and unit can now be corrected together on the same
activity without moving its time; local restore and encrypted two-device sync
check the result.
The selected child's birth date and growth-chart sex can also be corrected
without recreating the child or moving its activities. A blank date in this
edit keeps the previous value; local restore and encrypted sync cover changes.
The selected-child controls now include a direct action that scrolls to the
timeline below the logging forms. A 1.5× Android text-size check exposed that
scrolling to the screen's bottom landed in backup controls. The action now
targets the measured timeline heading, and the filter chips use two per row
so their labels stay readable at that size.
The timeline now groups entries under the viewer's local calendar day and
filters the selected child's entries by feeds, sleep, diapers, growth/care,
or notes. The Rust core still supplies the ordered activity projection;
filtering only changes the Android view.
An emulator UI pass hid a diaper under Feeds, restored it under All, and
showed its local calendar-day heading.
The same large-text check found a stale generic error after startup: a
superseded Compose load coroutine reported normal cancellation as a load
failure. Cancellation now ends that load silently; real failures still log
and show an error. Startup/navigation on the emulator no longer shows it.
Action results now also appear in a transient snackbar, so a failed log is
visible beside the current form without scrolling to the page's status text.
The oversized-bottle rejection showed that message while retaining its input
at 1.5× text size.
Completed whole-minute breast feeds now offer side and duration correction
for up to eight contiguous segments. The shared Rust core validates the
replacement as one atomic update and keeps the original start and activity
identity. A local restart/file-restore regression and a two-device relay
assertion cover the corrected projection; the Android headed UI smoke opens
and saves the correction dialog.
The full headed emulator UI smoke then passed creation, filtering, logging,
editing, deletion, and restart after the timeline and feedback changes. Its
keyboard helper now sends Back only while Android reports the keyboard open,
avoiding an accidental app exit when an input already dismissed it.
The Android logging screen also lets a caregiver choose an earlier local date
and time for the next completed entry. It records the offset at that instant
and returns to current time after a successful save; running sleep timers
still start now. A failed save retains the choice for retry. On an emulator,
a diaper saved for the previous day appeared at that time after process
restart, and the time control reset to current for the next entry.
Switching Family or child now clears unsaved logging inputs and pending edit
dialogs tied to the previous target. Bottle and completed-sleep inputs are
retained when a local save fails and cleared only after it succeeds; pumping
and solids submissions likewise leave newly typed values intact when an
earlier submission finishes.
An emulator check rejected an oversized bottle entry without losing its
draft, accepted the corrected amount, and cleared an unsaved note when the
caregiver switched to another child.
The diaper form now exposes the core's dry kind alongside wet, dirty, and
both, in two rows that fit larger text. The headed UI smoke logs a dry diaper
and reads it back from the timeline.
The top bar names the selected Family and child and offers a wet-diaper-now action,
so the common quick log is reachable without scrolling past setup and sharing
controls. It uses the current time while leaving a separately chosen
backdated-entry time intact. The headed UI smoke uses this action for its
first diaper, then corrects and deletes that same entry.
After the first child exists, the child-creation fields collapse behind an
explicit add-another-child action so daily logging starts higher on the
screen. A Family with no children still shows the initial setup form; the
headed UI smoke creates two children in its second Family.

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
The same local flow then clears the recipient installation, issues a new
invitation from the manager at epoch two, and verifies later-device claim,
challenge, proof, grant, hydration, encrypted upload, and manager readback.
Android chooses the next manager join action from verified pending state,
and its invitation control selects a later issue after the first one. The
later-device emulator path uses the automatic manager and recipient advance
calls for challenge, proof, and grant, with separate instrumented wake steps.
Creating a later invitation now first syncs the manager's current view and
pending edits, so a caregiver does not have to run a separate manual sync
before tapping Invite.
The join screen now names the verified stage: waiting for a holder challenge,
preparing the recipient proof, waiting for a holder grant, or loading history.
The later-device emulator flow checks the pending and proved phases.
The recipient UI now needs one Start or retry join action after a link opens.
It immediately runs one coordinator pass, then foreground polling and scheduled
work advance proof and history loading without separate buttons. A ready
joined Family enters the normal tracker. The duplicate debug child logging,
history, and backup panel is gone; a joined manager can invite from the
selected Family's access section, and a joined device can retry sync there.
The join card remains a developer link-entry and pending-stage surface until
the ordinary sharing UI gate. A focused real-relay emulator UI check opens a
fresh invitation link, taps that one action, and verifies the recipient claim
is committed and saved; the coordinator suites cover the later handshake.
Deep links place this join card first until history is ready, so the one
action and pending stage are visible without searching through the tracker;
the focused Compose UI check asserts it is visible and taps it without
scrolling. The prefilled invitation input stays one line so a long link
cannot push that action below a short viewport; opening the link also
returns the tracker scroll to the top. A 1080×1600 emulator check exercises
the action. The invitation-cancel UI check uses Compose semantics to find
and scroll to its action. Required CI run `36381384108` passed the 14-test
relay suite and tracker/recovery UI smokes on its second attempt; the first
attempt lost ADB during the tracker smoke after relay tests passed.
An admitted manager now exposes an invitation action in the selected Family's
access section and uses the same foreground and scheduled coordinator to answer
claims and proofs. A real-relay instrumentation case uses three independent
stores: the first manager admits a second manager, that manager invites and
grants a third device, then uploads an encrypted child the third device
reads. This is an emulator flow with one app installation and separate
Family credentials; distinct physical phones remain untested.
The admitted manager can also remove that third device. The Android test
checks the rotated membership, a verified removed status on the third
device, and continued sync by the original manager. A saved removal no
longer appears as an active shared snapshot in the tracker; the private
copy path still has access to its retained records.
The access list can give each verified device a label stored on this phone;
the removal control uses that label and the confirmation still shows the
complete device ID. Labels are only a local recognition aid, not proof of
which person holds a credential or a change to Family authority.
Ready joined Families now appear in the normal Family switcher and use the
same full tracker, timeline, edits, backup, and analysis export as the
initial manager Family through the shared Rust API. A keyless pending join
stays outside that switcher. The three-store relay case checks this
boundary and selects the third device's ready Family alongside a separate
local Family; the headed local UI smoke remains the presentation check.
Family choices now include the first child's name when known, so caregivers
can distinguish multiple local and joined Families without relying only on
their creation order. Empty Families retain the numbered label.
The tracker now remembers the active Family and child across a process
restart. Its emulator UI smoke creates a second Family with two children,
records on the second child, restarts, and checks that child's timeline.
The early independent authority recheck passed for that implemented cohort
at `82c0f4c`; [003](003-m0-foundation.md#early-m0-authority-recheck-at-82c0f4c)
records its limits. No physical two-phone or full M0 exit gate has passed.
Sharing setup and join controls are still debug UI with manual relay origin
and public key. Android's share sheet sends a one-use `babytrack://join` link;
opening it or receiving a raw text fragment prefills the join form without
claiming it. A focused emulator instrumentation test covers both paths.
This is still a development flow, not a released product flow.

## Ordered slices and gates

### 1. Local caregiver flow on one phone

- [x] Create and switch local Families, add/select children, log bottle and
  diaper, and inspect a persisted timeline through the shared Rust core on
  an emulator. The UI smoke runs after restart in CI.
- [x] Log sleep timers/completed sleep, notes, growth in whole grams and
  millimetres, and entered Celsius decimals through the local/shared core.
- [ ] Complete the MVP logging set: growth percentile display and practical
  edits beyond child name and growth details, note, bottle amount, diaper type, solids, growth,
  pumping amounts, medication, completed sleep, and temperature. Multi-segment breast
  feeds, note, bottle, diaper, and solids correction, and
  single-activity deletion now pass
  local restart/file restore, two-emulator encrypted sync, and UI tap flows.
  Preserve unknown fields and exact Family/child targets. Keep clinical
  recommendations out of the UI.
  WHO publishes the growth reference tables, but the terms for bundling those
  tables in this MIT app need resolution before adding percentile data. The
  [WHO standards](https://www.who.int/tools/child-growth-standards/standards)
  are available, but the [dataset terms](https://www.who.int/about/policies/publishing/data-policy/terms-and-conditions)
  restrict commercial-product use and the [copyright guidance](https://www.who.int/about/policies/publishing/copyright)
  calls for permission in a commercial context. Resolve the specific table's
  rights or obtain permission before bundling reference data; continue growth
  logging and editing meanwhile.
- [ ] Run local logging, Family switching, and restart on a physical Android
  phone. Check accessible labels, large text, dark night use, startup size
  and latency, and the common feed/diaper path with one hand. On 2026-09-28,
  the local ADB list and machine-control inventory offered only emulators,
  with no physical Android phone available for this gate.

Gate: the physical-phone flow works offline without an account, saves to the
intended Family and child, and remains usable after process death.

### 2. Two-caregiver sharing and background progress

- [x] Exercise first manager and one recipient on separate emulators through
  invite, claim, challenge, proof, grant, encrypted sync, delayed foreground
  passes, scheduled work, and first recipient removal/private copy.
- [ ] Complete admitted-manager membership actions, manager/role changes,
  invite cancellation, and key rotation in [003](003-m0-foundation.md) before
  exposing those actions as ordinary Android sharing controls. An admitted
  manager can now issue an invitation, complete challenge and grant, and
  remove another device at epoch one in the debug flow. A manager can now list
  and cancel unused invitations from the selected Family's debug access
  section, with a confirmation dialog; a real-relay emulator case clicks it
  and checks that the link becomes unusable. Managers can promote and demote
  admitted devices from this view; a real-relay case verifies the changed
  recipient role and denial of a later invitation after demotion. Two
  successive key rotations now pass a real-relay case. Pending-device
  removal and admitted-manager cancellation coverage remain open. A removed
  original manager now has a signed proof and private-copy path through the
  Rust binding and Android coordinator. The tracker omits that removed source
  from writable Family selection and offers a copy action that reuses an
  existing removal copy. The same card now appears for a removed recipient;
  a real-relay emulator case makes a copy with no pending edits and checks
  that its held child is present. A focused UI tap check remains open.
- [ ] Make invitation handoff usable through a share/link flow with trusted
  relay pinning and clear pending, accepted, blocked, and removed states.
  The share-sheet receive and custom-link paths now prefill without
  auto-claiming. Remove debug-only gating after the M0 exit security review
  passes. The join card now distinguishes saved recipient Families that are
  still joining from those with verified usable history, including after
  activity recreation. A confirmed claim consumes the incoming link so it
  does not prefill again on recreation. A real-relay UI test commits a claim,
  recreates the activity, and checks the pending label and cleared fragment.
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
  explicit presentation of data saved after the file point. An emulator now
  saves a readable file through Android's document picker, logs an activity
  after that save, sees the unsaved-changes warning, uninstalls the app, and
  restores the file into a fresh installation. The saved activity returns,
  the later activity does not, and the restored Family survives restart. The
  same check saves a password-protected file through the picker, reinstalls,
  rejects a wrong password without creating a Family, then restores with the
  correct password. The emulator picker check now also rejects a truncated
  readable file with a specific message and no Family creation. Physical
  phones and provider failures remain open. Remote run `36356278973` reached
  the protected-file preview but its UI script did not scroll to the
  confirmation button; a scroll-aware local rerun and remote run
  `36357854875` pass.
- [ ] Add the planned analysis export and Nara import through shared Rust
  import/export, with real sanitized samples and target preview. The
  current-state analysis CSV now exports local and shared Families through
  the core, including pending local shared edits; Nara import remains. No
  sanitized Nara export is available yet (confirmed 2026-09-27), so its exact
  column mapping and sample-backed validation remain open.
- [ ] Add an ongoing notification for timers and basic widgets without
  reimplementing event semantics in Android. Confirm stale actions after
  removal target a private copy or fail visibly. The Android sleep notification
  now reflects open timers across local and ready shared Families, opens the
  tracker on tap, and refreshes after foreground edits and scheduled sync.
  It requests Android's notification permission after a timer starts. The
  local journal start/stop notification case passes on an emulator. A boot
  receiver now rereads saved timers and restores the notification without
  waiting for network sync; the emulator test clears and restores that
  notification from the journal. A basic home-screen widget shows the number
  of saved running sleep timers and opens the tracker when tapped. It updates
  after tracker changes, scheduled sync, boot refresh, and launcher widget
  refresh; the emulator test checks its running and stopped renderings.
  An actual device reboot, widget placement on a launcher, and physical-phone
  checks remain.

Gate: caregivers can move a saved point to another phone and understand
what it contains; daily logging and recovery require no relay account.

## CI and review handoff

The Android CI job builds APKs. Its emulator job runs the real-relay
instrumentation suite, the command-driven local UI smoke, and the
document-picker recovery flow across fresh installations. Expand UI
checks for sharing and backup only where a failure would escape the core
integration suite. Record local emulator, physical phone, and observed
remote CI results separately. The M0 end review in [003](003-m0-foundation.md)
must pass before using real Family data. At the two-phone gate, report
observable flow status and unresolved risks to the user; routine code and
API decisions do not require a separate approval step.

Completion: the physical one-phone and two-caregiver gates pass, the planned
MVP tracking and recovery set is usable in Android developer builds, and
the relevant independent security gate and CI checks have recorded results.
