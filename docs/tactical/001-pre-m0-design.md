# 001: M-1 design closure

Status: complete, September 2026. This is the design milestone before M0
implementation; [003](003-m0-foundation.md) owns executable delivery.
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
  both retain local work, the removed device gets a private copy when work is
  pending, and other caregivers remain in the original Family. Record both
  orders and confirmation-loss cases.
- [x] Decide single-use direct join: creating the invitation is manager
  approval; no second manual approval, no reusable membership links.
- [x] Decide full portable file backup and restore, with optional protection,
  into a fresh local Family. Recreating the original shared group is not
  required; readable analysis exports remain available.
- [x] Decide D4-D5: no self-service original-Family recovery; grants, roles,
  and removals are per Family-specific device, separate from any account.
  Replacement requires an active manager's fresh invitation or a saved-file
  restore into a new Family.
- [x] Decide D8: unused invitation expires after seven days, can be
  canceled by an active manager, and ends when its issuer loses manager
  authority; an already committed enrollment remains pending.
- [x] Decide D7: retain work during access uncertainty; after verified
  removal, make one private copy for pending work or a new targeted action.
  With no work, offer an explicit copy. A member may deliberately copy a
  stranded Family after sole-manager loss.
- [x] Decide D6: show the deterministic field winner and retain the
  displaced edit in inspectable history; restoring it creates a new edit.
- [x] State the exact security limit for removing a lost device, excluding
  all known devices of a departed person, or facing an actively malicious
  manager; define the remaining-manager replacement path and sole-manager
  loss outcome.
- [x] Exclude automatic family and child merging from the MVP while keeping
  backdated logging and competitor CSV import.
- [x] Allow multiple Families in one app, with independent child histories
  and per-Family roles; use "Family" in the UI, not "sync group."
- [x] Specify explicit Family/child targets for new activity, pinned timer and
  widget/watch actions, selected import/export/invitation Families, and no
  fallback to another Family when a target is unavailable.
- [x] Exclude freezer milk inventory from the MVP.
- [x] Keep planned watch/widget basics in MVP milestones; paid unlocks are
  later extras and cannot gate Family data access.
- [x] Write concrete launch and dogfooding gates for two caregivers, offline
  logging, recovery, and export. Mark other surfaces as launch gates or
  explicitly later work.

### 2. Sync and security contract

- [x] Specify signed device batches and membership operations, authorized
  epoch transitions, and an atomic server compare-and-swap for rotation.
  Model concurrent removals, failed rotations, and stale clients.
- [x] Specify how a local-only family becomes shared: identifiers, initial
  manager authority, and whether all existing history is uploaded.
- [x] Specify what happens to offline writes made under an old epoch after a
  removal, including whether they can be re-encrypted by a remaining holder
  and whether a removed holder's late writes are rejected.
- [x] Specify the decided single-use direct-join protocol: atomic enrollment,
  authenticated retry by the same device, key handoff, and automatic
  asynchronous progress when a key holder and recipient sync at different
  times. Specify visible pending and delayed-wake fallback; M1 Android,
  M2 web, and M3 iOS validate platform background behavior without changing
  protocol authority. Do not require a second manual approval. Specify D8 expiry, cancellation,
  and issuer-removal races. Include concurrent redemption, preview, retry,
  replay, and leaked-link cases.
- [x] State what a malicious relay can hide or fork, what sequence numbers
  actually detect, and whether clients need an out-of-band history check.
- [x] Specify the lost-device and replacement flows under D4-D5: a new
  installation cannot reuse old authority; a remaining manager may invite
  it; without one, only locally held data or a saved full file can seed a
  new Family. Make the backup status and data-loss boundary explicit.
- [x] Specify the decided full-file backup/restore contract: readable and
  optionally protected forms, saved record state, metadata, versioning,
  credentials excluded, atomic restore into a new Family, and failure cases.
  Relay ciphertext or a lost device's key cannot reconstruct missing records.
- [x] Specify the verified-removal notice and D7 private-copy transaction.
  The new Family needs fresh identity and keys, must carry pending local
  work once, and must not merge with the original or retarget another Family.
- [x] Choose native SQLite with an append-only log and rebuildable current
  record projection, updated together in one transaction.
- [x] Specify atomic promotion/private-copy behavior; a crash must not
  strand or lose the original family history.
- [x] Specify canonical encoding, unknown-field preservation, API and batch
  version negotiation, limits, and downgrade behavior.
- [x] Specify local storage, sync, and backup isolation across multiple
  Families on one device, using separate device keys; add leakage tests.

### 3. Event and data contract

- [x] Choose distinct family metadata, child metadata, and child activity
  scopes in one operation log; tombstoning a child does not cascade-delete
  its activity history. Exact envelopes live in records v1.
- [x] State the exact meaning of last-writer-wins, including simultaneous
  edits to one field, clock skew, tombstones, and a way to inspect or restore
  an overwritten edit if the product promises no loss.
- [x] Keep breastfeeding segments as one field for the MVP; displaced edits
  remain inspectable under D6.
- [x] Choose stable source record IDs for overlapping imports where present;
  use a content identity and preview warning otherwise. File backup promises
  saved record state, not full source edit history.
- [x] Use viewer-zone midnight day boundaries for reports, with no
  configurable day start in the MVP. Include exact travel/DST vectors.

### 4. Validation and implementation handoff

- [x] Add symbolic user-flow scenarios with stable IDs, agreement status,
  observations, and variation coverage. These are not executable tests.
- [x] Define product-contract and independent security-review checkpoints
  before implementation, during M0, before real data, and at new boundaries.
- [x] Assign every agreed scenario to an ordered M0 slice; the scenario
  catalog owns its actions/assertions and M0 makes executable fixtures.
  Resolve proposals and assign UI coverage. Record exact
  runner commands when M0 creates them, not as fictitious M-1 test results.
- [x] Write `docs/protocol/` contracts and concrete `tests/vectors/` for settled
  protocol cases before M0 code, including join, remove, race, offline
  upload, recovery, unknown fields, and time boundaries.
- [x] List negative and adversarial tests beside each security guarantee.
  Include a relay that withholds the latest batch or presents two histories.
- [x] Define the M0 cross-platform proof: one encrypted event through Rust,
  Swift, Kotlin, and wasm before the core API is frozen, then a mixed-client
  exchange through the relay. Include a real-browser wasm/IndexedDB harness
  before the web product UI.
- [x] Define M0 slice gates for portability, local storage, real-relay sync,
  and adversarial membership/recovery. Require crash/restart and multi-Family
  isolation cases before M1 uses real data.
- [x] Review CI triggers and required checks against the shared-core and
  protocol/scenario/vector dependencies. Keep bounded deterministic protocol and relay tests
  on relevant PRs; run extended randomized tests and fuzzing nightly, with
  reproducible failure seeds. Keep performance baselines on physical devices.

## Decision lookup

- Product decision status: [D4-D8](../topics/family-sharing-and-trust.md#m-1-ux-decision-status).
- Protocol choices: [sharing](../protocol/sharing-v1.md),
  [records](../protocol/records-v1.md), and
  [portable file](../protocol/portable-file-v1.md).
- Deferred importer mapping: [event-model follow-up](../topics/event-model.md#open-questions).
- Relevant cases: [scenario index](../scenarios/README.md).
- Build preparation: [002](002-repository-scaffold.md); it does not satisfy
  this tactical's product/protocol gates.
- M0 implementation slices: [003](003-m0-foundation.md).

## Adversarial review record

Fixed protocol revision: `1571b77` on `main`. The independent Daybreak Blue
high-thinking sessions used the local Yep Anywhere API in read-only plan
mode. They assumed an honest relay for one ordered CAS history, single-use
and seven-day timing; they also tested an actively malicious relay that may
fork, withhold, or lie about its clock, an authorized hostile member, and
mixed minor-version clients. The accepted malicious-relay limits remain in
the Family sharing topic; the review did not prove an implementation secure.

| Finding cluster | Disposition and regression cases |
|---|---|
| Key delivered before admission; keyless claim could not commit | Public keyless claim/proof, holder-generated verifier challenge, atomic admission/grant; FS47, FS51, FS55, FS59 and contiguous chain. |
| Relay fork, forged authorship/removal, unknown upload result | Honest-relay scope explicit, per-device signed batches/receipts, signed removal chain and idempotent private copy; FS50, FS54, FS56-FS58. |
| Relay time, hostile HLC, backup identity, cross-minor malformed values | Signed commit-time expiry with accepted malicious-clock limit, log-position merge, stable scoped record IDs, frozen v1 validity; FS20, FS33, FS60-FS61 and byte vectors. |
| Missing transcript bytes, ordering, pre-create validity, metadata registry, object staging | Complete contiguous signed chain, unsigned-byte comparator, immediate inert pre-create batch, reserved record types, normal and genesis stage/commit/fetch fixtures; FS42, FS49, FS60. |

The early reviews failed while these defects remained. A focused review of
the fixed commit returned **PASS for the M-1 protocol/security gate**
(`01a0de33-3364-7502-8663-35b4ce9f797c`). The M0 review gates still
apply to code and real-family-data use.

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
