# 004: Autonomous delivery tracker

Status: active coordination; [003 M0 foundation](003-m0-foundation.md) owns
protocol work and [005 M1 Android](005-m1-android.md) owns the caregiver app.
This tracker records the next proof, gate
handoffs, and review evidence. The [MVP plan](../mvp-plan.md) owns scope and
milestone order; each implementation tactical owns its detailed checkboxes.

## Goal and exclusions

Keep progress visible from the M0 protocol foundation through a usable M1
Android UI, then hand off to the later surfaces in the MVP plan. Advance in
small, committed slices with executable flow evidence, proportionate CI, and
independent reviews at the named security gates. The user reviews flows and
architecture at milestone checkpoints; routine code and API choices stay
within the implementation loop. These checkpoints are progress reports, not
approval waits, unless a new product decision is genuinely required.

This tracker adds no product or protocol promises. It does not mark a gate
complete based on a design review, a passing build, or a model verdict alone.
Hosting, app-store work, and public release remain M5 work.

Delivery priority is the daily Android tracking, sharing, and recovery flow.
Use security review for broad direction at the named gates. Track narrower
edge cases for the gate or a later slice while feature work continues;
an access or data-retention failure against the agreed Family contract still
blocks real Family data.

## Current work card

M0 remains in progress. The implemented first cohort passes the early
authority recheck and focused general-authority advisory. Two separate
emulators exercise join, encrypted sync,
removal, and private-copy recovery. An admitted manager also invites and
grants to a third device, then removes it with key rotation, through relay
HTTP and Android instrumentation. The same manager removes the original
manager in a second rotation and continues writing at epoch three.
The removed original manager now verifies its own removal and saves pending
work into a private copy through the core. The tracker excludes removed
sources from writable Family selection and offers an access-ended copy action
for managers and recipients. A removed recipient without pending edits can
copy its held history and continue locally.
Pending-device removal, admitted-manager cancellation coverage, general browser shared
outbox, complete vector coverage, and the M0 exit review remain open.
The browser now persists a Rust-verified public control and accepted-batch
history in IndexedDB, with reload and forged-receipt rollback checks. It
also signs and decodes a bounded public pull. Chromium now fetches an
accepted encrypted batch through signed GETs from a disposable real relay
and reloads its saved cursor. It now fetches signed genesis objects and
projects the decrypted epoch-one record through Rust after reload. A
production wasm batch enters a durable browser outbox, previews locally,
survives a lost POST response and reload, and clears only on signed
acceptance. Later offline edits now queue behind that exact batch, preview
through Rust after reload, and seal one by one after signed acceptance.
The real-relay case uses two distinct dependent edits and loses a queued
upload response after acceptance; reload retries the exact saved envelope
before the remaining edit drains.
The browser data view also replays a same-epoch invitation after reload.
Later-epoch and recipient key storage, rejection repair, and full CLI/browser convergence remain
open. A test-only native CLI driver now verifies the browser's accepted
batch and sends an encrypted child back through the same relay; Chromium
projects it after reload. The first manager credential
persists in IndexedDB for this smoke.
Android local and first-cohort UI work, including solids, completed
multi-segment breast-feed, pumping logs, and note, bottle, diaper, pumping,
and medication corrections, plus child name and growth-detail, growth-entry,
and completed sleep corrections, runs
alongside M0 on emulators; its physical phone gates remain open.
Growth entry and correction now accept the published mass and length units,
default to kg/cm, and retain the entered form through local recovery and
two-device encrypted sync.
Known activities can now carry an editable optional note without losing their
type. The Android UI smoke attaches one to a growth entry across restart;
the two-device flow adds and clears one through encrypted sync.
The diaper form includes wet, dirty, both, and dry logging choices.
The top bar names the selected Family and child and has a wet-diaper-now shortcut above the long
logging form; the Android UI smoke uses it before correcting that entry.
After the first child, its creation fields collapse behind an add-another
action so common logging begins higher on the screen.
Running and completed sleep can carry an optional place through the shared
core, Android timeline, backup, and encrypted relay sync.
The Android tracker can also correct or clear that place on the same sleep
entry, including an open timer.
The two-device relay flow also caught a routine offline backlog above the
old sixteen-upload foreground cap. Android now attempts up to 64 uploads per
pass and keeps excess work pending for later sync; the test verifies this
flow drains before the second device reads the manager's new child.
Android can now share an invitation link through the system chooser. Opening
that link or receiving a raw fragment through the text share target prefills
an explicit join action without claiming automatically.
After that action, the Android coordinator makes an immediate pass and
continues claim, proof, grant, and history loading on foreground and scheduled
sync opportunities. The debug screen no longer asks caregivers to press
separate proof, history, challenge, or grant buttons.
The join card now labels a saved recipient as joining until its verified
history is usable, including after activity recreation; a confirmed claim
also consumes the launch link so it does not prefill again.
Ready joined Families now enter the normal Android Family switcher and full
tracker; pending joins remain outside it. The joined Family uses that tracker
for logging, backups, immediate sync retry, and a manager's next invitation;
the old duplicate debug child/history panel is removed.
A real-relay Android UI check now opens a fresh link, taps the single join
action, and verifies that its recipient claim was committed and saved.
The debug manager access section can list and cancel unused invitation links.
A real-relay UI test confirms the tap and signed cancellation; a separate
relay test checks exact preparation across reopen and another invitation
after a lost cancellation response.
The daily-use surface now has a saved-timer home-screen widget and local
labels for individual enrolled devices. Growth reference data licensing and
Nara sample mapping remain open for their planned features.
Caregivers can backdate one completed entry at a time. Bottle logging and
correction expose the four published content codes through the shared core;
the Android timeline and analysis CSV show that choice.
Completed whole-minute breast feeds can now be corrected for side and
duration through an atomic shared-core segment update.
The selected-child timeline groups activity by local day and filters its
view. Growth entries now carry optional head circumference through local
restore, encrypted sync, the Android form, and analysis CSV.
An Android emulator recovery check now saves readable and protected files
through the system picker and restores each after reinstall. It verifies the
saved point, rejects a truncated readable file and a wrong protected-file
password, and leaves the Family list untouched on either denial.
Physical-phone and provider-failure checks remain.
The Android tracker now retains the selected Family and child across process
restart, with a two-Family emulator UI check. Remote run `36356278973`
confirmed the concurrent SQLite fresh-open repair and passed the relay and
general UI paths. Its recovery check reached a protected-file preview, then
could not find the confirm button below the viewport. The picker test now
scrolls to that action; the complete local recovery flow and remote run
`36357854875` at `9f1fcd9` pass.

| Field | Current answer |
|---|---|
| Active implementation owner | [003 M0 foundation](003-m0-foundation.md) for protocol and [005 M1 Android](005-m1-android.md) for the caregiver app. |
| Next demonstrable proof | The Android local UI creates Families and children, records bottle, solids, completed alternating breast feed, diaper, sleep timer, completed sleep, note, growth, Celsius, and medication entries, shows a timeline with confirmed activity deletion, and saves/restores readable or password-protected files through Rust. A disposable relay and two separate emulators pass first-cohort join, a recipient timer stopped by the manager, recipient growth, temperature, medication, solids, and alternating breast feed read by the manager, reciprocal edits and one deletion, offline work, removal, and automatic private copy; note, medication, and solids markers are absent from relay storage and logs. Foreground polling and a scheduled job advance sharing; normal job timing is OS controlled. |
| Next dependent slice | The first-cohort recheck and [focused general-authority advisory](003-m0-foundation.md#general-authority-advisory-pass-at-98fa0ea) passed. Relay restart replays public controls and accepted batch authority; a signed private checkpoint covers rejected-result and staging-reservation sets. Signed GETs, batch POSTs, manager controls, and later control HTTP writes use that ledger under a SQLite writer lock. Later enrollment after removal and epoch rotation passes on separate emulators. A second admitted manager now invites, grants, and removes a third device through relay HTTP and Android instrumentation, then removes the original manager in a second rotation and writes at epoch three. An initial manager can cancel an unused invitation. An admitted manager can promote and demote an active device, and the recipient verifies the role after sync. Pending-device removal, admitted-manager cancellation coverage, browser outbox beyond the initial manager, and M0 exit review remain. |
| Next independent review | Run one broader end-of-M0 recovery and mixed-client gate after its executable coverage closes. Focused advisory findings remain open without serial rechecks during feature development. Prioritize usable tracking and browser shared sync before another review cycle. |
| Open advisory | The [automatic-status recheck](003-m0-foundation.md#advisory-automatic-status-recheck-at-ad42d78) found an own-claim lost-response ambiguity and a concurrent status-write race. FS37/FS77 regressions and repair remain due before real Family data. Durable streaming of large control history also remains open. |
| CI signal today | Rust, native-binding, browser, Android APK, real-relay emulator, local Android UI smoke, and document-picker recovery jobs are required on every push/PR. Run `36386643767` at `599937b` passed all required jobs, including queued browser edits, 14 Android relay tests, the tracker UI flow, and file recovery. Its preceding run found a transient SQLite lock in an independent UI-test assertion; the bounded assertion retry passed locally and on this run. Physical phones and the M0 exit gate remain open. |

Update this card when the active slice changes. Do not copy fine-grained
checklists from its owning tactical.
The first-run Android screen now shows separate create, join, and restore
actions; sharing setup opens on request so child logging stays reachable.
The join form remains visible for a saved pending recipient. A command-driven
headed emulator check creates a Family and child, logs and deletes a diaper
through the UI, and verifies both states after restart. CI runs this after
relay instrumentation.

## Ordered delivery handoffs

| Checkpoint | Owning work and exit evidence | Flow-level observation |
|---|---|---|
| M0 bytes and bindings | [003 slice 1](003-m0-foundation.md#1-bytes-and-bindings-before-core-api-freeze): exact and negative vectors in four runtimes. | One child event has the same verified meaning on every client boundary. |
| M0 durable local core | [003 slice 2](003-m0-foundation.md#2-durable-local-core): crash/replay, outbox, Family isolation, private copy, and backup/restore scenario results. | Local logging works offline; a saved copy names its Family and saved point. |
| M0 authority | [003 slice 3](003-m0-foundation.md#3-relay-authority-and-early-security-review): relay control tests and the early implemented security gate. | Invite, pending join, grant, removal, and key handoff have honest status and one committed outcome. |
| M0 mixed clients and exit | [003 slice 4](003-m0-foundation.md#4-mixed-client-sync-and-recovery-adversarial-pass): two CLI clients plus browser through a real relay, bounded fault suite, and end-of-M0 review. | Offline edits converge; rejected or unknown work stays visible; removal leads to a private copy when needed. |
| M1 first usable UI | [005 slice 1](005-m1-android.md#1-local-caregiver-flow-on-one-phone): Android local Family, child, feed/diaper logging, timeline, switcher, and backup on a phone. | A caregiver can log and inspect entries without an account or network. |
| M1 two-caregiver use | [005 slice 2](005-m1-android.md#2-two-caregiver-sharing-and-background-progress) and [two-caregiver gate](../mvp-plan.md#testing-and-validation): two phones, delayed join, offline edits, removal, private copy, and recovery. | The interface explains waiting, accepted, lost-access, and saved-copy states. |
| M1 refinements | [005 slice 3](005-m1-android.md#3-file-recovery-import-and-daily-use): timers, widget, import, accessibility, and daily developer-build use. | Common logging remains quick and correctly targeted to Family and child. |
| M2 web, M3 iOS, M4 watches | Open a tactical for each when approaching it; use the [milestone plan](../mvp-plan.md#milestones) and later-surface gates. | Reuse Family semantics; inspect each new platform trust and targeting boundary. |
| M5 distribution | Separate M5 tactical and release gate. | Public-use, hosting, legal, and store readiness are reviewed before release. |

These handoffs are a navigation view. Only the owning tactical's passing
evidence and completed checkboxes close an implementation gate.

## Security review queue

Use the [runbook](../security-review-runbook.md) with Daybreak Blue at high
thinking through Yep Anywhere. Commit the target first, review a fixed SHA,
record the session and findings in the owning tactical, add scenario/vector
regressions for substantive blockers, and recheck blockers at the named gate.
Focused advisories may be tracked to that gate without serial review loops.

| When | Review question | Gate owner |
|---|---|---|
| M0 byte/crypto preflight, run at `8402bd8` | Are the implemented wire, key, and binding boundaries safe enough to build on? Advisory FAIL; see the [record](003-m0-foundation.md#advisory-bytecrypto-preflight). | 003 slice 1; advisory only |
| M0 first-cohort preflight, run at `5ada80c` | Can a fresh join and promotion preserve eventual shared progress through retries and interleaving? Advisory FAIL; see the [record](003-m0-foundation.md#advisory-first-cohort-sync-review). | 003 slices 2–3; advisory only |
| Android sync preflight, run at `6b10805` | Can the Android outbox and enrollment recover across uncertain results and restarts, and show history gaps? Advisory FAIL; see the [record](003-m0-foundation.md#advisory-android-sync-review-at-6b10805). | 003 slices 2–3; advisory only |
| Android sync follow-up, run at `5e45858` | Did outbox rejection, recipient restart, and inert history visibility close? Advisory FAIL on two restart variants; see the [record](003-m0-foundation.md#advisory-android-sync-follow-up-at-5e45858). | 003 slices 2–3; advisory only |
| Android sync recheck, run at `00e827b` | Did the later-entry stale-epoch race and saved pre-commit claim restart close? Focused advisory PASS; see the [record](003-m0-foundation.md#advisory-android-sync-recheck-at-00e827b). | 003 slices 2–3; advisory only |
| Early M0 authority review, run at `2507537` | Can removed authors resolve their own uncertain upload result while new data access remains denied? FAIL; see the [record](003-m0-foundation.md#early-m0-implemented-authority-review-at-2507537). | 003 slice 3; formal gate open |
| Early M0 authority recheck, run at `fd4aca4` | Can a malicious relay use a signed result to falsely label delivery around a verified removal? FAIL; see the [record](003-m0-foundation.md#early-m0-authority-recheck-at-fd4aca4). | 003 slice 3; formal gate open |
| Early M0 authority recheck, run at `82c0f4c` | Did the saved cutover proof, ID checks, and GET route ACLs close first-cohort authority blockers? PASS for the implemented first cohort; see the [record](003-m0-foundation.md#early-m0-authority-recheck-at-82c0f4c). | 003 slice 3; later cohorts remain |
| General-authority seam advisory, run at `bb68156` | Can one public verifier safely serve client and relay for later devices? FAIL for the initial sketch; the [record](003-m0-foundation.md#advisory-general-authority-seam-review-at-bb68156) and [corrected topic proposal](../topics/sync-and-encryption.md#proposed-implementation-seam-for-general-relay-authority) require a historical ledger and transaction boundaries. | 003 slice 3; advisory only |
| Shared authority verifier advisory, run at `7566262` | Do the extracted issue, claim, challenge, proof, admission, and removal reducers preserve first-cohort client/relay agreement? Focused PASS; the [record](003-m0-foundation.md#advisory-shared-authority-verifier-at-7566262) requires complete reducers and epoch commitments before general routes. | 003 slice 3; advisory only |
| General-authority implementation advisory, run at `2461763` | Do ledger-backed general routes preserve access, transactionality, and availability under authorized hostile input? Advisory FAIL on pagination and a v1 object-manifest contradiction, plus reservation hardening; see the [record](003-m0-foundation.md#advisory-general-authority-implementation-review-at-2461763). | 003 slice 3; repaired and rechecked |
| General-authority recheck, run at `f55289a` | Did the first repair close those findings? FAIL on a batch-first object-reservation collision; see the [record](003-m0-foundation.md#general-authority-advisory-recheck-at-f55289a). | 003 slice 3; repaired and rechecked |
| General-authority recheck, run at `98fa0ea` | Does the batch-first repair preserve unrelated Family writes and the earlier fixes? Focused PASS; see the [record](003-m0-foundation.md#general-authority-advisory-pass-at-98fa0ea). | 003 slice 3; broader M0 exit remains |
| Later-invitation and paged-claim advisory, run at `a6ef118` | Can a later recipient claim from authenticated paged controls after unrelated authority writes? Focused FAIL on stale claim, unsigned terminal denial, and the Android history cap; see the [record](003-m0-foundation.md#advisory-later-invitation-and-paged-claim-review-at-a6ef118). | 003 slice 3; repair and fixed-revision recheck due |
| Later-invitation recheck, run at `137e1b1` | Did sparse-claim rebase and signed status close the prior findings? Focused FAIL on automatic action inference, Android terminal display, and terminal-cause history; see the [record](003-m0-foundation.md#advisory-later-invitation-recheck-at-137e1b1). | 003 slice 3; repair and fixed-revision recheck due |
| Automatic-status recheck, run at `3f19890` | Do automatic claim retry and first terminal cause remain correct after the previous fixes? Focused FAIL on automatic Android callers dropping verified terminal outcomes; see the [record](003-m0-foundation.md#advisory-later-invitation-automatic-status-recheck-at-3f19890). | 003 slice 3; durable status repair and recheck due |
| Automatic-status recheck, run at `ad42d78` | Does durable signed status remain honest across lost claim responses and concurrent callers? Focused FAIL on own-claim ambiguity and status insertion race; see the [record](003-m0-foundation.md#advisory-automatic-status-recheck-at-ad42d78). | 003 slice 3; track for M0 exit, continue independent MVP feature work |
| M0 exit, before real Family data | Can crash/retry, pending work, restore, or cross-Family access violate the agreed promises? | [End-of-M0 gate](../mvp-plan.md#security-review-gates), 003 slice 4 |
| New web/watch boundary and M5 | Does the new client or deployment boundary change the threat model or user-visible guarantees? | Later milestone tacticals and MVP plan |

## CI growth and evidence

The [current workflow](../../.github/workflows/scaffold.yml) checks Rust,
native bindings, wasm/browser storage, Android build and real-relay emulator
flow, dependency direction, and licenses. Its always-running required job
matches the current sharing surface, subject to remote-run verification. As
new main-branch commits arrive, workflow concurrency cancels superseded runs
so the newest revision receives the emulator gate without a backlog. When
the corresponding behavior exists, add local crash/replay and property tests,
broader bounded real-relay scenarios, nightly randomized/fault/fuzz runs with
saved seeds, and richer Android UI checks. Keep CI assertions tied to
implemented behavior; a green scaffold build does not establish sharing or
recovery correctness. Add path-based job selection and caching as the suite
grows, while core/protocol/vector changes still trigger every affected check.

Remote main run `36290799446` exposed infrastructure drift: the old Android
SDK setup action requested Google's removed `tools` package, so neither APK
nor emulator job ran; cargo-deny rejected `uniffi_bindgen` and `uniffi_udl`
at the already pinned UniFFI 0.32.2 version. The official setup action's
v4.0.4 release removes that package request. The two crate exceptions stay
limited to the pinned MPL-2.0 UniFFI build dependencies. A green remote run
is still required before marking CI healthy. Local `actionlint` and
`cargo deny --all-features check advisories bans licenses sources` pass for
this repair.

Remote run `36306915747` at `443c259` passed Rust, both native binding jobs,
browser wasm, and Android APK build. The real-relay Android job booted the
emulator and passed all seven instrumentation cases, then failed because the
runner lacks `rg` for the script's final result assertion. That assertion
now uses `grep -E`. Remote run `36307785165` at `ccfd5c4` passed the same
non-emulator jobs but exposed a `DatabaseBusy` collision in the scheduled-job
sharing test. The shared Rust local store now waits up to ten seconds for a
competing writer. The competing-connection Rust regression, seven real-relay
emulator instrumentation cases, one-emulator UI smoke, and ten-step
two-emulator sharing flow pass locally.

Remote run `36308749163` at `c640b15` then passed all required jobs,
including Android real-relay instrumentation, scheduled sync, and UI smoke.
The later restart-integrity commits have local server and two-emulator proof.

Remote run `36312981789` at `9a82ba7` passed all non-emulator jobs but one
scheduled Android instrumentation case opened the app database repeatedly
while the background job was still using it, producing `DatabaseBusy`. The
test now waits for the job's completion marker before inspecting its saved
claim. The seven-case real-relay instrumentation suite passes locally with
that ordering. Runs `36314170105` at `2461763` and `36314404419` at
`0b0c1f5` passed every required job.

Run `36314729319` at `99d2e75` exposed a different `DatabaseBusy` during
activity recreation while a separate shared-store connection was opening.
The local store had been running schema setup and an `INSERT OR IGNORE` on
every open. It now initializes an unversioned database once under a write
transaction and opens an initialized database without schema writes. A Rust
test holds a separate SQLite writer while the initialized store opens and
reads; all seven real-relay Android instrumentation cases pass locally with
the change. The next remote run must confirm this contention repair.

Remote runs `36309186029` at `88380d8`, `36309474621` at `148d53b`,
`36309921404` at `26227ca`, `36310142071` at `654f553`, and
`36310529356` at `bdbed9e`, `36311536009` at `6fc1928`, and
`36311858889` at `c195175`, and `36312097144` at `efc7b07` passed every required job, including the
real-relay emulator. The public authority replay and shared client binding
commits also have local Rust and two-emulator proof.

Run `36393612637` at `39b5edb` passed Rust, native bindings, browser, and
Android APK jobs, but the Android relay job failed one UI assertion. The
single-test run had one saved recipient, while the full 14-test suite had
several, so the new pending-join check assumed the wrong chip number. The
test now finds and selects its own visible saved recipient. All 14 relay
instrumentation cases pass together locally after that correction; the
next remote run must verify it.

Run `36397125159` at `8ad4297` passed all non-emulator jobs but again found
the join assertion sensitive to viewport position after activity recreation.
The target chip existed below the CI emulator's viewport; the test now
scrolls to it before checking visibility and selection. The complete
14-case relay suite passes locally with this change. A new remote run is
still required.

Run `36400724335` at `1afc2fd` passed Rust, browser, native bindings, Android
APK, and all 14 real-relay Android tests. Its headed tracker smoke stopped
while searching for a head-circumference value inside the scrollable growth
edit dialog on the CI emulator. The script now scrolls inside that dialog.
A shorter local emulator viewport exposed that the script read the physical
screen size instead of its active override; the helper now uses the effective
size. A 1080×1600 local replay passed the child shortcuts, growth correction,
and attached-note checks, then was stopped during later sleep checks already
covered by the earlier full-height run. The next remote run remains to be
verified.

For each gate, the owning tactical records the fixed commit, local commands,
observed CI run, scenario IDs, security session/disposition if applicable,
and remaining limits. Mark an unobserved remote run as unverified.

## Working loop and completion

1. Read the active tactical and relevant topics/scenarios; select its
   smallest incomplete proof.
2. Implement and test the full vertical behavior, including failure and
   restart paths when relevant; update contracts and vectors together.
3. Commit a coherent change and update its tactical status/evidence in that
   change. Check CI results when available; investigate failures.
4. At a security gate, run the independent reviewer and disposition findings
   before advancing to dependent work.
5. Update this card and the [tactical index](README.md) at handoff. Prepare
   a flow-level demonstration at M0 mixed-client exit and M1 phone checkpoints.

This coordination work is complete when the M1 functional UI and
two-caregiver gate pass and the next surface has its own tactical. Later
milestones continue under the MVP plan and new tacticals.
