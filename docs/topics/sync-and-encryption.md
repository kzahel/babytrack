# Sync and encryption

Status: proposed design, September 2026. M-1 must settle the open security
contract before M0 code.
Owns the operation log, merge rules, keys, invites, removal, recovery, server
API, protocol versioning, and threat model. The event payloads themselves are
owned by [event-model.md](event-model.md).
User-visible promises, accepted trust limits, and the agreed first-committed
outcome for competing removals are owned by
[family-sharing-and-trust.md](family-sharing-and-trust.md). Protocol choices
must implement that contract and its scenarios.

## Goals

- A new family works locally without an account or relay. Sharing is opt-in.
- Every authorized shared device holds the family history and works offline.
- Concurrent edits from any number of devices converge. Distinct event
  creations remain distinct; simultaneous writes to one field need an
  explicit policy for exposing the displaced value.
- The server stores and relays ciphertext only. It cannot read events,
  children's names, or anything else a family logs.
- After a removal commits, the removed member receives no new epoch key and
  cannot read entries encrypted under later epochs. Data already on their
  device remains. An honest relay rejects old-epoch uploads after cutover;
  a malicious relay hiding the change from a stale writer can expose that
  writer's old-key ciphertext to a removed holder of that key.
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
  Competing removals use the first valid relay-committed change; a stale
  request cannot exercise revoked authority. An offline request or missing
  response must not be presented as a completed change. Both managers may
  explicitly make independent copies, and unaffected members stay in the
  original Family. See the product contract's D1 and scenarios FS07-FS12.
- **Removed or departed.** Once the client learns that its membership has
  ended, it clearly says that sync for this family has stopped. It does not
  erase local history. The user may keep the old copy as an archive or
  continue privately from a copy with a new family identity and keys. That
  copy starts local-only and can be shared as a separate family later; it
  does not silently rejoin or merge with the original.

## Invites, removal, and recovery

- **Invites.** A manager authorizes one enrollment and its role when creating
  the invitation. A QR code or remote link carries single-use bootstrap
  access; remote secrets belong in the URL fragment. No second manual
  manager approval is required. The previous raw epoch-key payload is
  superseded: it must not allow link holders to bypass consumption and read
  history independently of enrollment. M-1 must specify the key handoff.
  The joining device generates its own keys; the relay atomically consumes
  the invitation with enrollment. Concurrent redemptions cannot create two
  enrollments. Previews do not consume it, and failed/lost responses allow
  authenticated resumption only by the same enrolled device. A fresh device
  needs a fresh invitation. Product decision D2 accepts that an unintended
  bearer may redeem a valid unused invitation first.
- **Invite lifecycle.** Expiry, cancellation, and outstanding invitations
  after their issuer loses manager authority are proposed in product D8.
  Settle their ordering with enrollment and key delivery before implementation.
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
- **File backup and independent restore.** Product D3 requires a complete
  restorable data file with optional password protection. This applies to
  local-only and shared Families. Restore creates a new local-only Family
  with fresh identity and keys, saved records, and no original membership or
  device credentials. It works without the original relay or manager. The
  person can share the new Family and reinvite caregivers; recreating the
  old shared group is not required for this recovery path. A key or phrase
  without a record copy is insufficient after device loss. The event topic
  specifies the file contract; original-Family key recovery remains separate.

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
`tests/vectors/`.

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
record backup. File backups may be readable by user choice; protected files
also require their protection credential. A hosted web client can be served
malicious code; see the plan's web client trust note.

Removal protects later-epoch ciphertext, not all records with an activity
or creation timestamp after the removal. A stale authorized writer may still
encrypt with an old key; a malicious relay can disclose that ciphertext to
a removed holder. This is an accepted product limit, not a claim that such
relay behavior is detectable. Honest-relay cutover and client authorization
checks are still required.

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
- File loss-recovery tests: readable and protected backups restore saved
  data into a new independent Family without original shared access. An old
  backup never reinstates revoked membership. Also verify that no-backup
  states do not claim lost records are recoverable.
- Sync torture test with a real server and many simulated clients going
  offline and reconnecting.
- Plaintext marker test: known strings written into events never appear in
  the server database or logs.
- Run the agreed [sharing scenarios](../scenarios/family-sharing-scenarios.json)
  with both race orders and lost-response/restart cases. Resolve proposed
  cases before claiming M-1 closure; these are not executable tests yet.

## M-1 candidate: removal cutover

The product's first-valid-commit outcome for competing removals is decided.
The following mechanism is still a candidate to test. Sign every batch with
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
2. **Single-use direct join.** D2 decides direct join without a second
   manual approval. Specify atomic consumption, device-bound authenticated
   resumption, role binding, and key delivery without a reusable raw Family
   key in the link. Determine background inviter availability requirements.
   Resolve D8 expiry, cancellation, and issuer-removal races. Test concurrent
   redemption, previewing, retries, and unintended-first-recipient cases.
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
12. **File backup implementation.** D3 settles optional protection and
    restore into a new independent Family. Specify the atomic restore and
    protection format with the event topic. Plain files are an intentional
    portability feature; they must omit original shared credentials. A
    phrase restores a key, not missing data. State backup completeness and
    snapshot time accurately.
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
