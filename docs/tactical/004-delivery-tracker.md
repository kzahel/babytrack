# 004: Autonomous delivery tracker

Status: active coordination; [003 M0 foundation](003-m0-foundation.md) owns
the current implementation work. This tracker records the next proof, gate
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

## Current work card

M0 remains in progress. Canonical bytes, selected crypto and operation
vectors, an in-memory projection, four-runtime encrypted-event smokes,
browser IndexedDB fixture smoke, native signed-history replay, and the
genesis, invitation, claim, challenge, proof, and admission development relay routes run locally. The full vector
suite, complete local storage, shared control and batch sync, recovery,
and sharing UI remain open; the first Android local tracking screen now runs
on an emulator. The current passing checks do not close an M0 slice.

| Field | Current answer |
|---|---|
| Active implementation owner | [003 M0 foundation](003-m0-foundation.md), slices 2 and 3; slice 1 cross-language gate is still open. |
| Next demonstrable proof | The Android local UI creates Families and children, records bottle, diaper, sleep timer, completed sleep, and note entries, shows a timeline, and saves/restores readable or password-protected files through Rust. A disposable relay and two separate emulators pass first-cohort join, a recipient timer stopped by the manager, reciprocal edits, offline work, removal, and automatic private copy; a note marker is absent from relay storage and logs. Foreground polling and a scheduled job advance sharing; normal job timing is OS controlled. |
| Next dependent slice | The early first-cohort authority recheck passed. Expand later-device authority, browser outbox, signed ID-collision coverage, and crash campaigns before M0 exit. |
| Next independent review | The [early M0 authority recheck at `82c0f4c`](003-m0-foundation.md#early-m0-authority-recheck-at-82c0f4c) returned PASS for the implemented first cohort. The broader end-of-M0 recovery and mixed-client gate remains. |
| Open advisory | The [Android sync recheck](003-m0-foundation.md#advisory-android-sync-recheck-at-00e827b) passed its two prior high findings at fixed revision `00e827b`. Activity recreation and forced recipient job retry pass on the real-relay emulator. Injected hostile-batch display proof and full M0 authority review remain. |
| CI signal today | Rust, native-binding, browser, Android APK, and real-relay emulator jobs are now required on every push/PR. The emulator runner passed locally against a relay, and `actionlint` passed. A remote Actions result has not been verified here. |

Update this card when the active slice changes. Do not copy fine-grained
checklists from its owning tactical.

## Ordered delivery handoffs

| Checkpoint | Owning work and exit evidence | Flow-level observation |
|---|---|---|
| M0 bytes and bindings | [003 slice 1](003-m0-foundation.md#1-bytes-and-bindings-before-core-api-freeze): exact and negative vectors in four runtimes. | One child event has the same verified meaning on every client boundary. |
| M0 durable local core | [003 slice 2](003-m0-foundation.md#2-durable-local-core): crash/replay, outbox, Family isolation, private copy, and backup/restore scenario results. | Local logging works offline; a saved copy names its Family and saved point. |
| M0 authority | [003 slice 3](003-m0-foundation.md#3-relay-authority-and-early-security-review): relay control tests and the early implemented security gate. | Invite, pending join, grant, removal, and key handoff have honest status and one committed outcome. |
| M0 mixed clients and exit | [003 slice 4](003-m0-foundation.md#4-mixed-client-sync-and-recovery-adversarial-pass): two CLI clients plus browser through a real relay, bounded fault suite, and end-of-M0 review. | Offline edits converge; rejected or unknown work stays visible; removal leads to a private copy when needed. |
| M1 first usable UI | Create an M1 tactical when M0 is near exit. Prove Android local Family, child, feed/diaper logging, timeline, switcher, and backup on a phone. | A caregiver can log and inspect entries without an account or network. |
| M1 two-caregiver use | M1 tactical and [two-caregiver gate](../mvp-plan.md#testing-and-validation): two phones, delayed join, offline edits, removal, private copy, and recovery. | The interface explains waiting, accepted, lost-access, and saved-copy states. |
| M1 refinements | M1 tactical: timers, widget, import, accessibility, and daily developer-build use. | Common logging remains quick and correctly targeted to Family and child. |
| M2 web, M3 iOS, M4 watches | Open a tactical for each when approaching it; use the [milestone plan](../mvp-plan.md#milestones) and later-surface gates. | Reuse Family semantics; inspect each new platform trust and targeting boundary. |
| M5 distribution | Separate M5 tactical and release gate. | Public-use, hosting, legal, and store readiness are reviewed before release. |

These handoffs are a navigation view. Only the owning tactical's passing
evidence and completed checkboxes close an implementation gate.

## Security review queue

Use the [runbook](../security-review-runbook.md) with Daybreak Blue at high
thinking through Yep Anywhere. Commit the target first, review a fixed SHA,
record the session and findings in the owning tactical, add scenario/vector
regressions for substantive findings, and rerun after fixes. A focused
preflight can reduce rework but never substitutes for a named gate.

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
| M0 exit, before real Family data | Can crash/retry, pending work, restore, or cross-Family access violate the agreed promises? | [End-of-M0 gate](../mvp-plan.md#security-review-gates), 003 slice 4 |
| New web/watch boundary and M5 | Does the new client or deployment boundary change the threat model or user-visible guarantees? | Later milestone tacticals and MVP plan |

## CI growth and evidence

The [current workflow](../../.github/workflows/scaffold.yml) checks Rust,
native bindings, wasm/browser storage, Android build and real-relay emulator
flow, dependency direction, and licenses. Its always-running required job
matches the current sharing surface, subject to remote-run verification. As
the corresponding behavior exists, add local crash/replay and property tests,
broader bounded real-relay scenarios, nightly randomized/fault/fuzz runs with
saved seeds, and richer Android UI checks. Keep CI assertions tied to
implemented behavior; a green scaffold build does not establish sharing or
recovery correctness. Add path-based job selection and caching as the suite
grows, while core/protocol/vector changes still trigger every affected check.

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
