# Family sharing and trust

Status: agreed product boundaries with explicitly proposed UX decisions,
September 2026. M-1 remains in progress. Scenarios are specifications, not
passing tests; no implementation exists yet.

Owns the user-visible meaning of local work, sharing, access changes,
independent copies, and recovery. The [sync topic](sync-and-encryption.md)
derives the protocol and security mechanisms from these promises; the
[event-model topic](event-model.md) owns record and editing semantics.
An implementation must not invent a new product promise to resolve a
technical difficulty. Bring back the concrete user-visible tradeoff.

## Agreed product boundaries

- **U1: Local work belongs to its holder.** Create a Family, log, read, and
  export without an account or network. Loss of shared access does not erase
  locally held records or pending edits. An independent copy permits further
  local work without the old Family's permission.
- **U2: Sharing is explicit.** First sharing includes existing history.
  Joining on first launch does not create an unwanted empty Family.
- **U3: Roles govern the shared space.** Members read and write; managers
  also manage access. No owner hierarchy or quorum. Any manager may request
  another manager's removal or demotion. The shared Family retains a manager.
  Conflicting requests need a defined outcome; see D1 below.
- **U4: Online confirmation governs shared access.** Local recording remains
  available offline. A membership request is not a completed access change
  until confirmed by the shared system. A timeout is not proof of failure.
  The relay coordinates shared access and ordering; clients also verify
  authorization. The relay receives no readable family content or data key.
- **U5: Removal ends shared participation, not possession.** Existing local
  history remains available. The person can explicitly continue with an
  independent local-only Family and share that copy later. Copies do not
  silently rejoin or merge. Nobody needs manager permission to make a copy
  of data already held locally, including a still-authorized manager.
- **U6: Families remain separate.** Roles, children, records, keys, and sync
  are scoped per Family. A timer retains its original Family and child.
  Quick logging identifies its target; switching Families does not retarget
  an already-started action.
- **U7: Recovery needs records and access.** A phrase alone cannot recreate
  lost local-only records. Platform backup is conditional convenience.
  Do not describe a Family as recoverable without a usable recovery path.
  A complete file backup can restore saved data into a new local Family;
  restoring the original shared group is not required for that path. File
  protection is optional, and ordinary readable exports remain available.
- **U8: The app does not arbitrate relationships.** An authorized manager may
  act against another manager. A reader can retain, export, or disclose data
  already received. We do not promise to prevent either behavior.

## Accepted trust limits

| Situation | Product promise and accepted limit |
|---|---|
| Relay storage is stolen | Entry contents remain encrypted; membership-related metadata, timing, sizes, and network metadata may be exposed. |
| A member tries to manage access | Reject unauthorized membership actions, including direct requests outside the UI. |
| A device is removed | Protect subsequent new-key shared data; retain its old local copy. Removal cannot remotely make it forget. |
| Another device is offline during removal | It cannot learn the change instantly. Its local work is retained; shared acceptance is resolved on reconnect. |
| Relay hides or forks history | Sync can be disrupted and not every stale view is detectable. Do not promise universal fork detection or global agreement against a malicious relay. |
| Relay hides a removal from an authorized writer | Data the writer still encrypts with an old key is not protected from someone who already holds that key. The broad phrase "all data created after removal is protected" is not a guarantee. |
| Every copy of records is lost | Keys alone cannot recover them. A relay is not an unconditional backup guarantee. |
| Hosted web code is compromised | Malicious delivered code can compromise that browser's access. Encrypted server storage does not solve this. |

These limits do not permit silent data loss, accepting forged authority, or
claiming a failed/unknown action succeeded. Exact local device protection,
recovery authority, and cryptographic verification remain M-1 work.

## D1: Two managers try to remove each other

Decision: **agreed**. The first valid removal committed by the relay takes
effect in the original Family. Private copies remain optional for both
people. This fixes the user-visible outcome; the exact atomic rotation
mechanism remains owned by the sync topic.

Example with Alice and Bob as the only managers:

1. Both request the other's removal. Neither UI claims success prematurely.
2. If Alice's request commits first, Alice remains in the original Family.
   Bob's removal of Alice cannot subsequently commit using his former role.
3. When Bob reconnects, show that his access ended and his request did not
   take effect. Keep his local history and pending entries. Offer an
   independent copy and an archive/export path.
4. Alice can also choose an independent copy, as any local holder can.
   Neither person is automatically moved into a new Family by the conflict.
5. Reverse the commit order and the outcome reverses. Device wall clocks and
   which button was tapped first do not decide the result.

Timing may choose between these two outcomes. With an honest relay and
eventual delivery, clients must agree on the original Family's membership
for the same committed order.
Deterministic resolution does not require choosing the same person regardless
of network order. Offline clients can temporarily show their last known
state, clearly distinguished from a newly confirmed action.

Rejected alternative: automatically move both people into new Families.
This would need additional rules for third caregivers, later-arriving
requests, and a device that never reconnects. It also cannot be treated as
an atomic split across offline devices.

Always test the case with a third caregiver: they should not be silently
copied, removed, or redirected because two other people disagree.

## D2: Invitations authorize one direct join

Decision: **agreed**. A manager creating an invitation authorizes one
enrollment into that Family with its specified role. The recipient can join
without a second manual approval by the manager. There are no reusable,
perpetual membership links. Another enrollment requires a fresh invitation.

Single use means one successful enrollment, not one page view or HTTP
request. Opening a preview, scanning the QR code, or a failed connection
must not consume it. Concurrent redemption attempts cannot enroll two
devices. Once used, the invitation cannot authorize another enrollment.
A lost response must allow the same enrolled device to resume its result
without opening access to a second device; the link alone must not suffice
to impersonate that already-enrolled device.

Possession of an unused valid invitation is permission. An unintended
recipient who obtains it and redeems it first may join and see history.
Single use does not identify the intended person, and removal cannot retract
data already received. The invitation screen explains that it grants access
to existing history. This is the accepted tradeoff for direct join.

Recommended additional protections are expiry and manager cancellation;
their lifecycle details, including outstanding invitations when the creator
loses manager authority, remain D8. The protocol must enforce consumption
and authenticated resumption and must not embed a reusable raw Family key
that bypasses that boundary. The accepted onboarding direction is asynchronous:
after the recipient opens the invitation, key handoff and history download
advance automatically whenever the required devices can run and connect.
The inviter need not stay online or reopen the app at the same time as the
recipient; another authorized key-holding device may deliver the key. A
normal background-wake path needs no further human action. Device wake and
network availability can delay progress, so the app persists the attempt and
resumes it on the next opportunity rather than promising an instant join.
The UI distinguishes invitation/enrollment pending, waiting for a Family
key holder, verifying the grant and loading history, and ready for shared
use; it never calls a keyless pending device ready. Exact status wording and
platform background scheduling remain design and validation work. No second
manual approval is required or may be introduced implicitly.

## D3: Portable files restore into a new Family

Decision: **agreed**. The MVP offers full Family file backup and restore,
with optional password protection. Readable exports are a normal supported
use: CSV for analysis and a documented full structured format for portability
and restore. Users can save files to their chosen destination. Automatic
cloud backup is separate convenience work, not a prerequisite for file
backup or a reason to assume records are safely backed up.

Restoring a complete file creates a new, independent local-only Family with
the saved children, entries, and the metadata required to use them. This
works on a new phone, after device loss, or after losing shared access. It
does not require the original relay or permission from the original Family.
For a protected file, the user also needs the file's protection credential.
Only the data actually saved is recoverable; show the backup point clearly.

The new Family has fresh identity and keys. The restorer can continue
logging, choose to share it, and invite caregivers again. File restore does
not reinstate membership or manager rights in the original Family, import
its device credentials, or silently replace or merge any existing Family.
An old backup cannot bypass removal from a shared Family. Family-file
backup is distinct from the separate key-backup/recovery mechanism.

The [event model](event-model.md#import-and-export) owns the complete file
contract. It must round-trip the saved record state, including children,
needed settings/metadata, units, and unknown fields. CSV is an analysis
format and is not promised to be a full-fidelity backup. Full edit-history
retention, deletion representation, versioning, and the optional protection
format still require M-1 specifications and fixtures. No key or phrase by
itself recreates missing records.

The UI explains readable-file access briefly, without making encryption a
condition of export or restore. Encrypted relay sync remains unchanged.

A possible later CLI tool could combine selected records from multiple
exports into a new or selected Family. It must use the shared core's import
rules, make child mapping and duplicate/conflicting-record handling explicit,
and preview changes before writing. This is not live Family/membership merge
and is not an MVP dependency. Do not promise such a tool exists yet.

## Remaining UX choices

These are recommendations for discussion, not decisions hidden in fixtures.

| ID | User question | Recommended direction | Must settle |
|---|---|---|---|
| D4 | Can I recover access to the original shared Family? | File restore already guarantees a new independent Family with saved data. Specify the separate original-Family recovery policy without inferring manager rights from decryption; settle sole-manager loss and concurrent old/new devices. | M-1, before recovery protocol |
| D5 | Am I removing a person or one lost device? | Distinguish those actions. Removing a person must account for their devices and recovery paths. Exact identity model remains open. | M-1, before membership protocol |
| D6 | What happens to an edit displaced by another edit? | Make the displaced value inspectable and explicitly restorable; retain operations regardless. | M-1, before edit/conflict contract |
| D7 | A widget/watch targets a Family whose access changed; where does my tap go? | Preserve the explicit target and action, explain the problem, and offer a deliberate private-copy route. Never redirect into the active Family silently. | M-1 targeting contract; UI in M1/M4 |
| D8 | When does an unused invitation stop working? | Expire invitations, allow cancellation, and invalidate unused invitations when the creator loses manager authority. Choose the lifetime and race behavior explicitly. | M-1, before invitation protocol |

The exact wording/layout may evolve without changing these promises. Do not
present archive, pending-sync, access-ended, and independent-copy states as
interchangeable. In particular, a network outage alone is not proof of removal.

## Validation

The [scenario guide](../scenarios/README.md) indexes cases by concern and
owns coverage dimensions, runner expectations, and the link from product
promises to observations. Read the relevant cases, not the entire catalog
for every task. `agreed` means specified, not implemented or passing.

The [MVP plan](../mvp-plan.md#security-review-gates) owns review checkpoints.
A reviewer must map findings to product promises and scenario IDs. Technical
constraints do not silently override these UX decisions.

## Reconsider if

- Families need owner priority, quorum, or protection against hostile
  co-managers; this changes the authority product, not just a dialog.
- Users need different data visibility for different caregivers.
- The relay must be untrusted for global membership ordering as well as
  confidentiality; stronger coordination and different UX may be needed.
- Scenario tests expose a user-visible outcome not covered here. Record it
  before choosing protocol behavior implicitly.
