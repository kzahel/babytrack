# 001: M-1 design closure

Status: in progress. This is the design milestone before M0 implementation.
The [MVP plan](../mvp-plan.md) fixes the architecture requirements; the
[sync](../topics/sync-and-encryption.md) and
[event-model](../topics/event-model.md) topics own the detailed decisions.
This tactical tracks which decisions and examples must be complete before
code begins. A recommendation below is not a decision until its topic is
updated and its counterexamples are resolved.

## Goal

Make the data and sync contract precise enough to implement without choosing
security, merge, or recovery behavior ad hoc in M0. Keep the first usable
product small enough to validate with families.

Decided product behavior: new families start local-only; sharing is opt-in.
First launch can join an invitation instead. One app can hold and switch
between multiple independent Families, with Family as the user-facing term.
Each Family has its own children, history, role, device keypairs, and sync state.
The first share includes the family's existing history.
Shared families have managers (read/write plus membership management) and
members (read/write). Removed members retain their local data and may make
an independent local-only family, which they may share later. The design
assumes cooperating caregivers rather than trying to arbitrate hostile
co-managers. Any manager may remove or demote another manager, while at
least one manager must remain. Automatic merging of live families, child
identities, and membership histories is outside the MVP; backdated logging
and competitor CSV import remain in scope. Native storage is SQLite with an
append-only log and rebuildable current-record projection. M-1 still needs
exact protocol and UX rules for these cases.

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
- [ ] Decide invite authority and expiry. Compare direct epoch-key links with
  short-lived, inviter-approved remote invitations. Include replay and leaked
  link cases.
- [ ] State what a malicious relay can hide or fork, what sequence numbers
  actually detect, and whether clients need an out-of-band history check.
- [ ] Define recovery identity and key rotation after a lost device or
  phrase. Specify fallback when platform backup is unavailable, delayed, or
  not end-to-end encrypted.
- [ ] Define local-only record backup and restore. A saved key or recovery
  phrase cannot reconstruct a lost database without a ciphertext copy.
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

- [ ] Write `spec/` contracts and illustrative vectors for all settled
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
  `spec/` dependencies. Keep bounded deterministic protocol and relay tests
  on relevant PRs; run extended randomized tests and fuzzing nightly, with
  reproducible failure seeds. Keep performance baselines on physical devices.

## Questions to resolve first

| Question | Working direction | Decision record |
|---|---|---|
| Who controls membership? | Any manager may manage any other, but one manager must remain. Decide how permissions bind to devices and survive recovery. | [Sync topic](../topics/sync-and-encryption.md) |
| What does removal guarantee? | Protect future data after a committed rotation; document any limit against an actively hostile current member or relay. | [Sync topic](../topics/sync-and-encryption.md) |
| What if an offline client writes under the old epoch? | Keep the local write, then define acceptance or authorized re-encryption at reconnection. Never silently discard it. | [Sync topic](../topics/sync-and-encryption.md) |
| What is a safe remote invite? | Prefer a short-lived, approved join over a reusable link containing the epoch key; check usability against offline requirements. | [Sync topic](../topics/sync-and-encryption.md) |
| What restores a family after phone loss? | Treat platform backup as conditional convenience. Test phrase and second-device paths independently. | [Sync topic](../topics/sync-and-encryption.md) |
| What backs up local-only records? | Decide on encrypted file backup and restore, or explicitly disclose the risk of a sole unbacked device. | [Sync topic](../topics/sync-and-encryption.md) |
| Are metadata records activity events? | Separate record scopes while sharing operation and encryption machinery. | [Event-model topic](../topics/event-model.md) |
| Is freezer inventory core logging? | Treat it as individual stored portions with amount, storage date, and use/discard state if included; otherwise defer it explicitly. | [Event-model topic](../topics/event-model.md) |
| What does "no lost data" mean with LWW? | Promise convergence and retention of operations, then decide whether displaced edits are visible or recoverable. | [Sync topic](../topics/sync-and-encryption.md) |
| How does sharing begin? | Start local-only. On explicit opt-in, register the family and upload the existing history under encryption; settle id stability and exact UX. | [Sync topic](../topics/sync-and-encryption.md) |
| What does a removed member keep? | Preserve local history. Offer a separate private family with fresh keys; allow a later share of that new family. | [Sync topic](../topics/sync-and-encryption.md) |
| Can families or children be merged? | No automatic live merge in the MVP. Backdated entries and competitor CSV import remain; a later local import tool may copy selected records as new operations. | [Event-model topic](../topics/event-model.md) |
| Can one app hold multiple Families? | Yes. A caregiver can switch among independent Families with different roles; the same real child can have separate histories in two Families. Settle quick-log targeting and per-Family identity/storage. | [Sync topic](../topics/sync-and-encryption.md), [Event-model topic](../topics/event-model.md) |
| How is the timeline fast if the log only grows? | Query a rebuildable current-record projection, not the log. Update both in one SQLite transaction on native devices and test replay equivalence. | [Sync topic](../topics/sync-and-encryption.md) |

## Gates

- The two topic documents state each protocol and event decision as decided,
  or explicitly defer it with a milestone and proof that M0 compatibility is
  unaffected. No security-critical item above is left as an open question.
- A reader can walk through local creation, promotion to shared, join,
  concurrent edit, removal, private fork, multi-Family switch, backdated entry,
  offline reconnect, recovery, and export without inventing rules.
- The MVP plan, proposal, topic documents, and vectors agree about scope,
  security guarantees, and terminology.
- The M0 tactical can be written as ordered implementation slices with
  concrete tests, including a browser harness and mixed-client exchange,
  without needing a new product or protocol decision.

## Completion condition

Mark this tactical complete, and update the tactical index in the same
change, only when every gate holds. M0 starts afterward.
