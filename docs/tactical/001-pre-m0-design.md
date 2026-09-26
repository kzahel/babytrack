# 001: M-1 design closure

Status: in progress. This is the design milestone before M0 implementation.
The [MVP plan](../mvp-plan.md) fixes the architecture requirements; the
[Family sharing](../topics/family-sharing-and-trust.md),
[sync](../topics/sync-and-encryption.md), and
[event-model](../topics/event-model.md) topics own the detailed decisions.
This tactical tracks which decisions and examples must be complete before
code begins. A recommendation below is not a decision until its topic is
updated and its counterexamples are resolved.

## Goal

Make the data and sync contract precise enough to implement without choosing
security, merge, or recovery behavior ad hoc in M0. Keep the first usable
product small enough to validate with families.

The [Family sharing contract](../topics/family-sharing-and-trust.md) owns
U1-U8 and D1-D8. The technical topics own mechanisms. This tactical tracks
closure; it does not maintain a second copy of those decisions.

## Exclusions

- No app, server, protocol, or CI implementation in M-1.
- No hosting, store enrollment, publication, or production credentials.
- No attempt to freeze visual design or every later feature. A deferral must
  name its milestone and state why it cannot invalidate the M0 protocol.

## Ordered slices

### 1. Product and threat boundaries

- [x] Decide the product's local-only default, opt-in sharing, two roles,
  and the removed member's ability to continue from a private copy.
- [x] Decide that any manager may remove or demote another manager, without
  an owner hierarchy or quorum, while at least one manager must remain.
- [x] Record the high-level local/shared authority and accepted trust limits
  in the Family sharing topic, without requiring product decisions in terms
  of cryptographic machinery.
- [x] Decide mutual-removal behavior: first valid relay commit takes effect;
  both retain local work, copies are optional, and other caregivers remain
  in the original Family. Record both orders and confirmation-loss cases.
- [x] Decide single-use direct join: creating the invitation is manager
  approval; no second manual approval, no reusable membership links.
- [x] Decide full portable file backup and restore, with optional protection,
  into a fresh local Family. Recreating the original shared group is not
  required; readable analysis exports remain available.
- [ ] Resolve user-visible invitation lifecycle, original-Family recovery
  authority, person/device removal, displaced edits, and stale quick-log
  targeting (D4-D8 in the Family sharing topic).
- [ ] State the exact security limit for removal of a lost device, departed
  member, or actively malicious manager, and how manager authority is
  restored after device loss.
- [x] Exclude automatic family and child merging from the MVP while keeping
  backdated logging and competitor CSV import.
- [x] Allow multiple Families in one app, with independent child histories
  and per-Family roles; use "Family" in the UI, not "sync group."
- [ ] Specify the Family switcher and target Family/child behavior for logging,
  timers, widgets, notifications, watches, import/export, and invitations.
- [ ] Reconcile first-class watch and widget promises with the proposal's
  possible paid unlock. Decide whether freezer inventory is in the MVP.
- [ ] Write concrete launch and dogfooding gates for two caregivers, offline
  logging, recovery, and export. Mark other surfaces as launch gates or
  explicitly later work.

### 2. Sync and security contract

- [ ] Specify signed device batches and membership operations, authorized
  epoch transitions, and an atomic server compare-and-swap for rotation.
  Model concurrent removals, failed rotations, and stale clients.
- [ ] Specify how a local-only family becomes shared: identifiers, initial
  manager authority, and whether all existing history is uploaded.
- [ ] Specify what happens to offline writes made under an old epoch after a
  removal, including whether they can be re-encrypted by a remaining holder
  and whether a removed holder's late writes are rejected.
- [ ] Specify the decided single-use direct-join protocol: atomic enrollment,
  authenticated retry by the same device, key handoff, and background inviter
  availability. Settle expiry, cancellation, and issuer-removal races (D8).
  Include concurrent redemption, preview, retry, replay, and leaked-link cases.
- [ ] State what a malicious relay can hide or fork, what sequence numbers
  actually detect, and whether clients need an out-of-band history check.
- [ ] Define recovery identity and key rotation after a lost device or
  phrase. Specify fallback when platform backup is unavailable, delayed, or
  not end-to-end encrypted.
- [ ] Specify the decided full-file backup/restore contract: readable and
  optionally protected forms, saved record state, metadata, versioning,
  credentials excluded, atomic restore into a new Family, and failure cases.
  A saved key or phrase alone cannot reconstruct missing records.
- [ ] Define the revoked client's notice and private-copy path. The new
  family needs fresh identity and keys and must carry forward pending local
  entries without silently merging with the original.
- [x] Choose native SQLite with an append-only log and rebuildable current
  record projection, updated together in one transaction.
- [ ] Specify atomic promotion/private-copy behavior; a crash must not
  strand or lose the original family history.
- [ ] Specify canonical encoding, unknown-field preservation, API and batch
  version negotiation, limits, and downgrade behavior.
- [ ] Specify local storage, sync, and backup isolation across multiple
  Families on one device, using separate device keys; add leakage tests.

### 3. Event and data contract

- [ ] Give family settings, children, and child activity records consistent
  envelopes and merge rules. Activity records must retain child scoping;
  family and child metadata need not pretend to be child activity.
- [ ] State the exact meaning of last-writer-wins, including simultaneous
  edits to one field, clock skew, tombstones, and a way to inspect or restore
  an overwritten edit if the product promises no loss.
- [ ] Decide whether breastfeeding segments need independent operations;
  settle freezer inventory if it remains in the MVP.
- [ ] Specify stable importer identity across overlapping exports and the
  round-trip contract for state versus full operation history.
- [ ] Confirm time-zone and day-boundary behavior with travel examples and
  decide whether family-level reporting needs a stable time zone.

### 4. Validation and implementation handoff

- [x] Add symbolic user-flow scenarios with stable IDs, agreement status,
  observations, and variation coverage. These are not executable tests.
- [x] Define product-contract and independent security-review checkpoints
  before implementation, during M0, before real data, and at new boundaries.
- [ ] Turn every agreed scenario into concrete M0 fixtures/actions/assertions
  in the handoff plan; resolve proposals and assign UI coverage. Record exact
  runner commands when M0 creates them, not as fictitious M-1 test results.
- [ ] Write `docs/protocol/` contracts and concrete `tests/vectors/` for settled
  protocol cases before M0 code, including join, remove, race, offline
  upload, recovery, unknown fields, and time boundaries.
- [ ] List negative and adversarial tests beside each security guarantee.
  Include a relay that withholds the latest batch or presents two histories.
- [ ] Define the M0 cross-platform proof: one encrypted event through Rust,
  Swift, Kotlin, and wasm before the core API is frozen, then a mixed-client
  exchange through the relay. Include a real-browser wasm/IndexedDB harness
  before the web product UI.
- [ ] Define M0 slice gates for portability, local storage, real-relay sync,
  and adversarial membership/recovery. Require crash/restart and multi-Family
  isolation cases before M1 uses real data.
- [ ] Review CI triggers and required checks against the shared-core and
  protocol/scenario/vector dependencies. Keep bounded deterministic protocol and relay tests
  on relevant PRs; run extended randomized tests and fuzzing nightly, with
  reproducible failure seeds. Keep performance baselines on physical devices.

## Decision lookup

- Product choices: [D4-D8](../topics/family-sharing-and-trust.md#remaining-ux-choices).
- Protocol choices: [sync open questions](../topics/sync-and-encryption.md#open-questions).
- Record/format choices: [event-model open questions](../topics/event-model.md#open-questions).
- Relevant cases: [scenario index](../scenarios/README.md).
- Build preparation: [002](002-repository-scaffold.md); it does not satisfy
  this tactical's product/protocol gates.

## Gates

- The owning topic documents state each product, protocol, and event
  decision as decided, or explicitly defer it with a milestone and proof
  that M0 compatibility is unaffected. No security-critical item above is
  left as an open question.
- A reader can walk through local creation, promotion to shared, join,
  concurrent edit, removal, private fork, multi-Family switch, backdated entry,
  offline reconnect, recovery, and export without inventing rules.
- The MVP plan, topic documents, and scenarios/vectors agree about scope,
  security guarantees, and terminology.
- Sharing scenarios distinguish agreed outcomes from unresolved proposals.
  All M-1 UX choices have a disposition; no proposed fixture becomes an
  implicit implementation decision. Reviewer findings map back to scenario
  IDs, including both outcomes of manager races and lost confirmations.
- The M0 tactical can be written as ordered implementation slices with
  concrete tests, including a browser harness and mixed-client exchange,
  without needing a new product or protocol decision.

## Completion condition

Mark this tactical complete, and update the tactical index in the same
change, only when every gate holds. M0 starts afterward.
