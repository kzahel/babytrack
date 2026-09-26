# Sync and encryption

Status: proposed design, September 2026. M-1 must settle the open security
contract before M0 code.
Owns the operation log, merge rules, keys, invites, removal, recovery, server
API, protocol versioning, and threat model. The event payloads themselves are
owned by [event-model.md](event-model.md).

## Goals

- A new family works locally without an account or relay. Sharing is opt-in.
- Every authorized shared device holds the family history and works offline.
- Concurrent edits from any number of devices converge. Distinct event
  creations remain distinct; simultaneous writes to one field need an
  explicit policy for exposing the displaced value.
- The server stores and relays ciphertext only. It cannot read events,
  children's names, or anything else a family logs.
- After a removal commits, the removed member receives no new epoch key and
  cannot read later shared entries. Data already on their device remains.
- A shared family can recover from phone loss when another complete copy or
  relay history plus a valid key recovery path remains. Local-only families
  need a separate data backup; a recovery phrase alone cannot recreate lost
  records.
- Clients on different app versions interoperate without dropping data.
- One app can participate in several independent Families without a global
  account; a removal or sync failure in one does not affect the others.

## Operation log and merge

Working direction; edge cases below remain open for M-1.

- The unit of change is an **operation** on one record: `create` (id, type,
  fields), `set` (id, field, value), or `delete` (id). Every operation
  carries a hybrid logical clock (HLC) stamp: wall-clock milliseconds, a
  counter, and the writing device's id as the final tiebreak. HLC stamps give
  every operation a total order that respects causality on each device.
- **Merge** is last-writer-wins per field by HLC. This converges but displays
  only one of two concurrent values for the same field; the operation log
  retains both. `delete` sets a tombstone
  field, so a later `set` of the tombstone can undelete. A `set` on an event a
  replica has not seen yet is kept and applied when the `create` arrives.
- Operations are grouped into **batches** for upload. Each batch has a
  client-generated id (so retried uploads are idempotent), the writing
  device's id, and a per-device sequence number inside the ciphertext so
  other members can detect a gap the server has hidden.
- The append-only log is the source of truth. A rebuildable projection holds
  the current state of each record, including the winning stamp per field
  and tombstones. The UI queries that projection rather than replaying the
  log for every screen. Native SQLite writes an accepted operation and its
  projection change in one transaction; web IndexedDB uses an equivalent
  transaction. The projection can be rebuilt by replaying the log. No
  log-compaction or snapshot protocol is required in M0; measure replay
  before adding snapshots.

## Keys

Working direction, subject to the M-1 authority and recovery decisions.

- **Epoch keys.** Batches are encrypted with a random 256-bit epoch key using
  XChaCha20-Poly1305. The associated data binds the family id, epoch number,
  protocol version, batch id, and device id, so a ciphertext cannot be
  replayed into another family or epoch.
- **Key holders.** Devices, platform backups, and recovery phrases can hold
  grants to epoch keys. Devices have signing and agreement keypairs and can
  act in the shared family. Platform-backup and recovery-phrase holders are
  for restoring access, not for issuing ordinary requests or inheriting a
  manager role automatically; M-1 must specify how a restored device is
  authorized. Backup private material lives in iCloud Keychain
  (synchronizable) or Block Store; recovery material derives from 24 words.
- **Membership** is recorded inside the encrypted log as signed operations:
  a device joined (with its public keys, role, and a display name), changed
  role, or was removed. Clients verify signatures and the signer's authority
  against their own membership view. The two product roles are manager
  (read/write plus membership management) and member (read/write only).
- **Grants.** A new epoch key is wrapped for each current holder with HPKE
  (RFC 9180, X25519, HKDF-SHA256, ChaCha20-Poly1305) and stored on the server
  addressed by an opaque holder id.
- **Keyring.** Each rotation also stores the list of all earlier epoch keys
  encrypted under the new one, so a newly joined holder can read the full
  history while a removed one gains nothing.
- **Server auth.** The earlier proposal to derive one auth keypair from the
  shared epoch key cannot enforce manager-only actions: every reader knows
  that key. M-1 must define device-specific request authorization and the
  minimum opaque membership metadata the relay needs to reject unauthorized
  joins, removals, and role changes. The relay still receives no data key.

Per-holder keypairs are needed because a removed device that knows the old
shared key could otherwise read the new key while it is handed to the
others. Roles limit use of the app's shared family; they cannot stop a
member who can decrypt all data from copying or exporting that data.

## Local, shared, and detached families

The product calls each independent tracking and sharing space a **Family**.
"Sync group" describes an implementation boundary, not a UI term. One device
can hold multiple Families, each with its own identity, log and projection,
membership and role, keys and epochs, relay endpoint, and sync cursor. A
person can be a manager in one and a member in another. Device signing and
key-agreement identities are generated separately for each Family. There is
no global account, device identity, or cross-Family manager. Ciphertext
batches and exports are scoped to one Family. A future explicit client-side
import may copy selected records
into another Family as new operations. Separate keys do not prevent a relay
from correlating Families through IP addresses or push tokens. M-1 must
specify how local storage partitions and backups enforce this boundary.

- **Local-only by default.** A new family logs through the same core model
  without contacting the relay. The user explicitly chooses to share it.
  First launch may instead join an invitation without creating an empty
  local Family. The user may create further local Families later.
  The first share includes its existing history. M-1 must settle whether
  its family id and operation ids remain unchanged when this history is
  first uploaded.
- **Shared.** A manager enrolls another device as a manager or member. Both
  roles can read, log, edit, and export the locally held family data. Only a
  manager can change membership. Any manager may remove or demote another
  manager, but a shared family must retain at least one manager. There is no
  owner hierarchy or approval quorum. An ordinary member still has enough
  data to create an independent copy outside the shared family.
- **Removed or departed.** Once the client learns that its membership has
  ended, it clearly says that sync for this family has stopped. It does not
  erase local history. The user may keep the old copy as an archive or
  continue privately from a copy with a new family identity and keys. That
  copy starts local-only and can be shared as a separate family later; it
  does not silently rejoin or merge with the original.

## Invites, removal, and recovery

- **Invite in person.** The inviting device shows a QR code carrying the
  server URL, family id, current epoch, and epoch key. A manager must
  authorize the new holder and role. The new device generates its keypairs
  and receives grants like any other holder. M-1 must revise the direct-key
  payload if it bypasses manager approval.
- **Invite remotely.** The same payload in the URL fragment of a link. The
  link is as sensitive as the key, so the app warns before sharing it.
- **Removal.** A manager can remove a holder. The removing device creates
  epoch e+1, grants it to every remaining holder, posts the keyring and the
  new authorization state, and appends a signed "removed" operation. The
  relay rejects subsequent requests from the removed holder. M-1 defines
  how that holder learns of removal without access to the new epoch.
- **Recovery for shared families.** A new device that has the recovery phrase
  or restores the platform backup derives or loads that holder's keypairs,
  fetches its grant and ciphertext history, and joins as a new device.
  Rotating the phrase or backup is a removal of the old holder plus a new
  join. The mechanism for restoring manager authority is still open.
- **Recovery for local-only families.** A key or phrase without an encrypted
  copy of the local records is insufficient after device loss. M-1 must
  define whether the MVP offers an encrypted backup/export and restore path
  or explicitly warns that unbacked local-only history can be lost.

## Server API

A relay with no knowledge of event content. Sketch, to be fixed in the M0
spec:

- register a local family for sharing (family-id rule and initial manager
  authorization fixed in M-1);
- append a batch (idempotent by batch id; server assigns a sequence number);
- list batches after a sequence number;
- put and get grants and the keyring for an epoch;
- rotate the epoch and update opaque membership authorization atomically;
- a WebSocket for live notification of new batches;
- register a push token and send empty wake pushes on new batches.

The push sender is an interface with APNs and FCM implementations first and
UnifiedPush later (see the plan's service interface requirements). Quotas
limit storage per family and requests per family and IP.

## Encoding and versioning

- Operations and batch payloads use CBOR with integer keys. Only the Rust
  core reads the format, so no cross-language schema tooling is needed.
- Unknown event types and unknown fields are kept verbatim and written back
  unchanged. An old client never drops what a newer one wrote.
- Every batch header carries a protocol version. Minor versions are
  additive. A client that sees a newer major version keeps reading what it
  can, stops writing, and asks the user to update.

## Crypto implementation

Pure-Rust RustCrypto crates (`chacha20poly1305`, `hkdf`, `x25519-dalek`,
`ed25519-dalek`) and an HPKE crate, so the same code runs natively and in
wasm. No custom primitives. Every construction gets known-answer vectors in
`spec/`.

## Threat model

The server, or anyone who compromises it, **can see**: which shared families
exist, how many holders each has (from grant addressing), opaque role and
authorization metadata needed to enforce membership rules, when batches
are written and how large they are, epoch rotations, client IP addresses,
and push tokens. Activity timing reveals when a family is awake. Local-only
records and keys are never sent to the relay; a hosted web app still has to
download its code.

The server **cannot** decrypt event content from a batch whose epoch key it
does not hold. Under the current AEAD-only batch sketch, a member with that
key could claim another device's id. Per-device batch signatures are the
M-1 candidate to prevent that impersonation; the final authorship guarantee
depends on settling and testing that rule.

A malicious server **can**: withhold, delay, or delete batches, or serve a
stale or forked view. Per-device sequence numbers detect gaps between known
batches; they cannot detect a withheld latest batch without another source
of history. A device retaining a local copy can survive relay deletion, but
recovery after loss of every device also depends on relay data or a separate
encrypted backup. A hosted web client can be served malicious code; see the
plan's web client trust note.

## Validation

- Property tests: random operations delivered in random orders to several
  replicas always converge to identical state.
- Projection tests: replaying the same log from scratch produces the same
  current records as incremental updates, including edits and tombstones.
- Known-answer vectors for every crypto construction and for encoding, run
  from every language binding.
- Rotation tests: a removed holder cannot decrypt batches from later epochs,
  and the server rejects its requests.
- Role tests: a member cannot change membership; a manager can remove a
  manager; removing or demoting the last manager is rejected.
- Recovery tests: phrase and platform backup both restore full history on a
  fresh device.
- Local-only loss test: restore from an actual data backup, or verify that
  the product clearly reports that no recoverable backup exists.
- Sync torture test with a real server and many simulated clients going
  offline and reconnecting.
- Plaintext marker test: known strings written into events never appear in
  the server database or logs.

## M-1 candidate: removal cutover

The following is a candidate to test, not a decision. Sign every batch with
its device key. Bind a manager-authorized rotation to the prior epoch and
membership state, and commit the grants and membership change atomically
with a compare-and-swap on the server. Reject uploads sealed under the old
epoch after that cutover. A remaining device that was offline keeps its own
pending operations locally, fetches its new grant, and uploads those same
operations in a new-epoch batch; the removed device cannot do that. An
unpublished old-epoch write by the removed device would not join the family
history after cutover. Tests must cover this case without silently losing
pending writes by remaining members.

This cutover serializes concurrent rotations but does not by itself enforce
roles. The current product direction is two roles and a basic system for
cooperating caregivers. The relay could hold opaque device public keys and
role grants, checking manager signatures for membership actions without
seeing family content. Clients must verify the same signatures rather than
trusting the relay's claim. This trusts the relay for ordering and
availability, though not for data confidentiality. Any manager can remove
another manager, subject to the at-least-one-manager rule. A malicious
manager may still act first; the product does not try to prevent a person
with an existing local copy from making an independent fork.

## Open questions

1. **Role identity and recovery.** Any manager may invite, remove, or change
   another manager's role as long as one manager remains. Decide whether a
   signed role grant applies to a person or an individual device and how
   manager authority is recovered after losing a device. A manager acting
   maliciously can still win a rotation race; the design accepts this limit.
2. **Remote invites with approval.** A safer remote invite would carry a
   short-lived invite key and require the inviter to approve the new
   device's keys. It needs the inviter online. Decide whether it replaces
   the plain link for the MVP.
3. **Encoding.** Confirm CBOR over protobuf once the core exists. The
   requirement is verbatim preservation of unknown fields.
4. **Web key storage.** The web client keeps its holder keys in IndexedDB.
   Decide whether to require a passphrase to unlock them on shared
   computers.
5. **Batch authorship.** Membership operations are signed, but ordinary
   batches are not yet specified as signed by the device. Decide whether
   every batch must carry a device signature and how membership at its epoch
   is checked.
6. **Rotation transaction and offline writes.** Define an atomic transition
   from epoch e to e+1 with grants, keyring, membership change, and auth key.
   Decide what happens to old-epoch writes created offline or uploaded while
   the rotation is in flight, including writes from the removed holder.
7. **Malicious relay visibility.** Decide whether a client needs an
   out-of-band history comparison or whether stale and forked views are an
   explicitly accepted limitation. Sequence gaps alone are insufficient.
8. **Recovery guarantee.** Platform backup may be unavailable or delayed;
   Android Block Store cloud backup is end-to-end encrypted only when its
   [availability check](https://developer.android.com/identity/block-store)
   succeeds. Decide what recovery state the UI reports and which fallback
   must be confirmed before a family relies on it.
9. **Concurrent same-field edits and clock skew.** Specify whether the UI
   exposes displaced values from the log, and bound or handle future-dated
   HLC stamps so one clock does not dominate later edits indefinitely.
10. **Local-to-shared promotion and fork.** Decide family-id stability,
    history upload, and whether the first share includes all local history.
    Define how a removed client learns of removal, how it makes a private
    copy with new keys and identity, what provenance is retained, and how
    pending offline edits enter that copy. No automatic merge with the old
    family is promised.
11. **Promotion and copy durability.** Native storage uses SQLite, with log
    and projection updates in one transaction. Specify transactions for
    promotion and private copy so a crash cannot mix family identities or
    drop pending local edits.
12. **Local-only backup.** Decide how a user restores records after losing
    their sole local-only device. A phrase restores a key, not missing data;
    a portable JSON/CSV export may be plaintext and is not automatically a
    secure backup.
13. **Multiple-Family isolation.** Use per-Family device keypairs. Specify
    storage and backup partitioning, and how joins choose a relay endpoint
    without a global account. Specify what happens when one Family is removed
    while another remains active. Separate keys alone do not prevent server
    correlation through network or push metadata.

## Reconsider if

- The log grows large enough that full replay is slow on a low-end phone:
  add encrypted client-generated snapshots.
- Families need to share a subset of data, such as a daycare seeing only
  today's feeds: this needs per-child or per-audience keys and read-only
  server auth.
- Users need roles such as read-only caregivers: symmetric epoch keys cannot
  enforce read-only, so writes would need per-holder signatures checked by
  every client.

## References

Signal (libsignal, Signal-iOS, Signal-Android) for device linking by QR and
per-device keys. Actual Budget for HLC-based sync through a relay. Ente for
recovery key UX. Automerge and Loro as the alternatives this design rejects
for being more general than the data needs.
