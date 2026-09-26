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
  also manage access. Grants and roles belong to Family-specific devices,
  not global users or accounts. No owner hierarchy or quorum. Any manager
  device may request another manager device's removal or demotion. The shared
  Family retains an active manager device. Conflicting requests need a
  defined outcome; see D1 below.
- **U4: Online confirmation governs shared access.** Local recording remains
  available offline. A membership request is not a completed access change
  until confirmed by the shared system. A timeout is not proof of failure.
  The relay coordinates shared access and ordering; clients also verify
  authorization. The relay receives no readable family content or data key.
- **U5: Removal ends shared participation, not possession.** Existing local
  history remains available. Confirmed removal with pending local work
  automatically creates one independent local-only Family that includes
  that work. With no pending work, the person can explicitly make a copy.
  Either copy can be shared later; neither silently rejoins or merges.
  Nobody needs manager permission to copy data already held locally,
  including a still-authorized manager or member.
- **U6: Families remain separate.** Roles, children, records, keys, and sync
  are scoped per Family. A timer retains its original Family and child.
  Quick logging identifies its target; switching Families does not retarget
  an already-started action.
- **U7: Recovery needs saved records.** A complete file backup can restore
  its saved data into a new local Family with fresh identity and keys. There
  is no self-service credential that restores original-Family access or
  missing records after the last authorized device is lost. Another active
  manager may send a fresh invitation to a replacement device. Do not
  describe a Family as recoverable without a usable saved copy. File
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
| The sole manager device is lost | Other devices may keep read/write access, but cannot invite or remove devices without manager authority. A member can deliberately copy locally held data into a new Family, become its manager, invite others, and stop using the old Family locally. Members cannot remotely revoke the lost manager credential or erase the old relay history. |
| Hosted web code is compromised | Malicious delivered code can compromise that browser's access. Encrypted server storage does not solve this. |

These limits do not permit silent data loss, accepting forged authority, or
claiming a failed/unknown action succeeded. Exact local device protection
and cryptographic verification remain M-1 work.

## Device access without login

Each installation creates its own Family-specific device signing and key
agreement identity. A manager grants a role to that identity through one
invitation; another phone or browser installation needs its own invitation.
"Device" means this credential, not a verified physical device or person.
If its private keys are copied, both copies may appear as the same device;
revoking that credential removes both. The MVP must protect local keys but
cannot promise hardware identity on every platform.
The relay sees opaque device authorization metadata, never a global account
identity or readable caregiver name. Display names and device labels help
people recognize access but are not proof that two devices belong to one
person. Managers grant, demote, and remove devices individually. The access
UI says **Remove device** and lists each enrolled device; it does not promise
one-tap person-wide revocation. A person who controls another still-authorized
device retains that device's access, and a manager can invite a replacement.

No login is required for local logging, sharing, or Family access. A future
account for licensing or paid services is separate from Family membership:
it cannot authorize a device, restore a role, or decrypt Family data.

## Family and child targeting

The phone shows an explicit active Family and child for new activity actions.
If either is required but missing, it asks for a target before saving. Starting
a timer captures both IDs; switching views later does not move that timer.
A configured widget captures its Family and child, and a watch action carries
the selected Family and child to the phone. Imports choose a destination
Family and child mapping before writing. Export, backup, and invitation
creation name the Family being acted on. An inaccessible or deleted target
never falls back to the currently visible Family or child. D7 governs work
already aimed at a Family whose shared access was later removed. Exact
control layout and labels may change without changing these target rules.

## D1: Two managers try to remove each other

Decision: **agreed**. The first valid removal committed by the relay takes
effect in the original Family. The race concerns the two named manager
devices. Other devices held by either person are not implicitly removed.
The removed device automatically receives a private copy if it has pending
local work; otherwise copying remains optional. This fixes the user-visible
outcome; the exact atomic rotation mechanism remains owned by the sync topic.

Example with Alice and Bob as the only managers:

1. Both request the other's removal. Neither UI claims success prematurely.
2. If Alice's request commits first, Alice remains in the original Family.
   Bob's removal of Alice cannot subsequently commit using his former role.
3. When Bob reconnects, show that his access ended and his request did not
   take effect. Keep his local history and pending entries. If he has pending
   work, save it in an automatic private copy; otherwise offer a copy. Keep
   the original as an archive/export source.
4. Alice can also choose an independent copy, as any local holder can.
   Neither person is moved into a new Family merely by the conflict; Bob's
   pending local work is the automatic-copy trigger.
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

On an honest relay's one ordered Family history, single use means one
committed pending enrollment, not one page view or HTTP
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
An actively malicious relay can fork the control history and consume the same
invitation differently on two hidden branches. Single use and first-committed
membership outcomes are honest-relay guarantees, not global agreement
against relay equivocation. Clients pin their observed control heads and
reject a detected sibling or rollback, but a hidden fork may remain unseen.

On an honest relay, invitations expire seven days after their committed
creation if still unused. A malicious relay controls its own signed clock
and may extend this real-time limit by future-dating the issue receipt; the
MVP has no independent time witness.
Any active manager can cancel an unused invitation. Unused invitations also
become permanently invalid when their issuer loses manager authority;
promoting that device again does not revive its old links. The protocol must
enforce consumption and authenticated resumption. It must not embed a raw
reusable Family key that bypasses that boundary. The accepted onboarding
direction is asynchronous:
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
An old backup cannot bypass removal from a shared Family. File backup is the
MVP's portable recovery path; it never carries original-Family authority.

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

## M-1 UX decision status

Decided rows are product requirements. The other rows remain proposals for
discussion, not decisions hidden in fixtures.

| ID | User question | Current direction | State |
|---|---|---|---|
| D4 | Can I recover access to the original shared Family? | **Decided:** no self-service original-Family recovery from a phrase, platform backup, file, or paid account. A still-active manager can invite a replacement device. Otherwise use an existing local copy or saved full file to create a new Family; unsaved records may be lost. | Agreed for M-1 |
| D5 | Am I removing a person or one device? | **Decided:** grants and removal are per Family-specific device. Show enrolled devices and revoke each one explicitly; labels do not establish person identity or person-wide removal. | Agreed for M-1 |
| D6 | What happens to an edit displaced by another edit? | **Decided:** the deterministic field winner is shown normally, while the losing edit remains inspectable in record history. Restoring it creates a new edit; neither value is silently deleted. | Agreed for M-1; exact merge rule in sync |
| D7 | Where does local work go when shared access changes? | **Decided:** while access is uncertain, retain work for its original Family; on verified removal, create or reuse one private copy for pending work or a newly attempted action and tell the user where it was saved. With no work, offer an explicit copy. Never retarget work to another open Family. | Agreed for M-1; UI in M1/M4 |
| D8 | When does an unused invitation stop working? | **Decided:** on an honest relay, seven days after committed creation, explicit cancellation by any active manager, or permanent invalidation when the issuer loses manager authority, whichever occurs first. An already committed enrollment remains pending through later expiry or issuer removal; managers may remove that pending device explicitly. | Agreed for M-1 |

D8 governs initial pending enrollment, not the time needed for asynchronous key
handoff. A preview never consumes the invitation. The relay's committed
ordering decides a race: redemption committed before expiry, cancellation,
or issuer-role loss is the one authorized enrollment; a later attempt is
rejected. A committed pending enrollment does not disappear merely because
seven days pass or its inviter is removed. A manager can separately remove
the pending device before key admission. Once admission and grant commit,
later removal uses normal rotation and cannot retract old history. The exact timestamp representation,
clock source, and atomic checks belong in the protocol contract.

D7 distinguishes uncertainty from confirmed loss of access. A network outage,
timeout, or unknown upload result leaves the original Family and its durable
outbox intact; the app neither announces removal nor makes an automatic copy.
After verified removal, a locally pending entry, edit, deletion, or timer
change triggers one private Family copy from locally held data, including that
work. A stale widget or watch action targeting the removed Family uses that
same copy, creating it if needed, and reports the destination. Repeated
delivery of the same action must not create another Family or duplicate the
entry. The original remains available as locally held history, not as a
shared writable destination. With no pending or new action, show the option
to continue privately without creating a Family the user may never use.

A still-authorized member whose sole manager device was lost can deliberately
copy locally held data to a new Family, become its manager, and invite others.
The old Family is not remotely dismantled; the user may stop using or archive
it locally. A copy must not claim to include history the device never
received. Show incomplete-sync status before copying when known, and never
claim the new Family is a continuation of the old shared authority.

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
