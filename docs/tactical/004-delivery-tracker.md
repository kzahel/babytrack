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
| Next demonstrable proof | The Android local UI creates Families and children, records bottle and diaper entries, shows a timeline, and saves/restores readable or password-protected files through Rust. A debug sharing preview uses Android Keystore and the native Rust state machine to promote a Family, issue a one-device invitation, and submit a recipient's keyless claim through a local relay. The emulator test passed exact retry of both manager writes and the recipient claim across separate stores. Key handoff, data readiness, edit sync, removal, and complete transport remain. |
| Next dependent slice | Shared current-state export and explicit idempotent private copy now work for native verified Families; the existing protection wrapper accepts that readable file. Automatic copy on removal, browser outbox, and crash campaigns remain before the M0 exit gate. |
| Next independent review | The required early M0 authority review follows implemented invite, grant, and rotation. |
| Open advisory | The [sparse-join follow-up](003-m0-foundation.md#sparse-join-repair-follow-up-at-7e4a8a6) passed the actual sparse and zero-gap routes but found an unused nonempty-batch helper with a crash gap. That helper is removed; the sparse route handles pre-invitation batches. Full membership authority, later epochs, and browser outbox remain before shared sync. |
| CI signal today | Read-only Rust, native-binding, browser, and Android APK jobs are required on every push/PR. Local checks passed; a remote Actions result has not been verified here. |

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
| After minimal authority implementation, before broader sync | Can an unauthorized, stale, or hostile device gain or retain shared authority or a new epoch key? | [Early M0 gate](../mvp-plan.md#security-review-gates), 003 slice 3 |
| M0 exit, before real Family data | Can crash/retry, pending work, restore, or cross-Family access violate the agreed promises? | [End-of-M0 gate](../mvp-plan.md#security-review-gates), 003 slice 4 |
| New web/watch boundary and M5 | Does the new client or deployment boundary change the threat model or user-visible guarantees? | Later milestone tacticals and MVP plan |

## CI growth and evidence

The [current workflow](../../.github/workflows/scaffold.yml) is a foundation
gate: Rust, native bindings, wasm/browser storage, dependency direction, and
licenses. Its always-running required job is useful at this maturity. As
the corresponding behavior exists, add local crash/replay and property tests,
bounded real-relay scenario tests, nightly randomized/fault/fuzz runs with
saved seeds, and then Android build/UI checks. Keep CI assertions tied to
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
