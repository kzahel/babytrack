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
  manager role automatically; D4-D5 must specify whether and how a restored
  device is authorized. Possessing or decrypting a recovery grant is not an
  authorization-state entry. Backup private material lives in iCloud Keychain
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
  or restores the platform backup may derive or load recovery material and
  fetch an addressed grant and ciphertext history. That does not yet make the
  new device an authorized member or manager. Enrollment of the replacement,
  the disposition of the old device/credential, and restoration of any role
  remain open in D4-D5. A protocol must not describe that path as recovery of
  original-Family access until those decisions are made.
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
- stage idempotent encrypted promotion chunks and atomically activate their
  signed manifest;
- append a batch (idempotent by batch id; server assigns a sequence number);
- list batches after a sequence number;
- put and get grants and the keyring for an epoch;
- commit invitation creation/redemption, grant activation, and epoch rotation
  with opaque membership authorization atomically;
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
- Recovery tests: once D4-D5 settle the authority policy, each accepted key
  recovery path is tested with available relay history, delayed/unavailable
  backup, and revoked credentials. Until then, no test may imply that a phrase
  or platform backup restores membership, manager authority, or missing
  records merely because it decrypts an old grant.
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

## M-1 candidate: authority and key handoff

**Review proposal, not a settled wire contract.** This maps the agreed U1-U8,
D1-D3, and FS cases to one implementable direction. D4-D8 remain product
choices; in particular this candidate cannot promise person-wide revocation
or original-Family recovery until D4-D5 settle their identities and holders.
The exact canonical encoding, domain separators, request authentication,
failure recovery, and vectors still belong in `docs/protocol/` before M0 code.

### Authority and commit order

- Each shared Family starts with a manager device's signing and agreement
  public keys and a signed genesis authorization state. A public, ordered
  control chain contains only opaque device/holder IDs, their public keys and
  roles, epoch and state version, invitation commitments/status, and hashes
  of encrypted objects. Names, records, and data keys stay encrypted.
  Genesis also commits to the first random epoch key. Existing clients pin
  genesis and their latest accepted head, reject rollback or a sibling of
  that head, and verify every transition. A first-joining client pins the
  Family ID, relay endpoint, genesis hash, invitation ID, invited role, and
  hash of the manager-signed invitation from the fragment bootstrap
  descriptor; the fragment's invitation secret authenticates that descriptor
  but is not a Family key. This prevents a relay from replacing the Family,
  genesis, role, or invitation transcript during first contact. It does not
  make a relay-created fork from a previously valid head universally
  detectable; that remains the accepted threat-model limit.
- A manager signs a transition over Family ID, prior state hash/version,
  a unique transition ID, intended new state and epoch, and the complete
  manifest of encrypted objects. Creating an invitation is itself such a
  transition: the current manager commits its ID, public key, exact role, and
  signed invitation hash before distributing the descriptor. An authorized
  invitation redemption is a narrow exception to a current-manager signature:
  the committed invitation delegates exactly one pending enrollment and its
  later activation at the fixed role. The invitation key and new device both
  sign a redemption transcript bound to the Family, current head, invitation,
  role, device signing and agreement keys, and their key versions. A current
  key holder and that same device later authorize only the grant/activation
  steps described below; neither may change the delegated role or another
  membership entry. The relay accepts a transition only if its expected prior
  hash/version is current, signatures and roles are valid, the last manager
  remains, and all required objects are present. Only active managers count
  for the last-manager invariant; a pending invited manager cannot be used to
  remove or demote the last active one. One durable compare-and-swap commits
  its new state, encrypted membership operation, any rotation grants and
  keyring, and resulting cursor.
  A timeout is resolved by querying the signed transition ID and verifying
  the exact committed state hash and objects; it is never treated as proof of
  failure or success. A losing manager request is not automatically replayed:
  it must still be authorized and have the same meaning against the winning
  state before its device signs a replacement. In the D1 race, the removed or
  demoted manager cannot do so.
- Every ordinary batch has a stable `(author device ID, operation ID)` for
  each operation, separate from record ID, HLC, batch ID, and epoch. The
  decrypted operation author must match the signed batch author. The device
  signs a canonical envelope containing Family ID, state hash/version, epoch,
  protocol version, batch ID, its device ID and sequence, and the ciphertext
  hash. The relay
  requires the referenced authoring state to be an ancestor of its current
  head, the envelope epoch to equal the current epoch, and the device still to
  be active with write authority at the atomic commit position. Thus an
  unrelated invitation transition need not invalidate Charlie's offline
  batch, while a batch from Charlie received after Charlie's removal is
  rejected even though its authoring state was once valid. Clients verify the
  same signature, ancestry, epoch, and authorization at the batch's committed
  position; they deduplicate operation IDs when the same pending operation is
  rebatched.
  HLC still chooses field winners; it is not an operation identity or an
  authorization clock. A retry of an accepted batch returns its first result.
  A device with an unknown upload result first queries or pulls by its stable
  batch ID; it does not sign a different batch using the same device sequence
  while that result is unknown. Re-encryption after a definite old-state
  rejection uses a new batch ID and the next admissible device sequence while
  retaining operation IDs. Exact committed-sequence allocation and rejection
  receipts must be fixed in the wire contract so a normal rejected attempt is
  not mistaken for a relay-hidden committed batch.

### Rotation, grants, and offline work

- For removal or demotion that changes key access, the manager generates a
  fresh random epoch key. First it commits to that key in a transition core
  containing a domain-separated key commitment, Family, prior state, new
  epoch, and intended new authorization state. It uses the core hash as HPKE
  context while wrapping the new key separately to each remaining authorized
  key recipient and encrypting the earlier-key keyring under the new key. The
  signed manifest binds the transition ID and core, crypto suite, recipient
  holder/device IDs and agreement-key versions, and hashes of **every** grant,
  keyring, and encrypted membership operation. The keyring contains numbered
  earlier epoch keys; after decrypting it, a client checks every key against
  the corresponding commitment in its pinned chain and rejects omissions or
  substitutions. This two-stage construction avoids a circular hash.
  A recipient checks the entire signed manifest and control chain, decrypts
  only its addressed grant, checks the new-key commitment and prior-key
  continuity, then activates the epoch. A relay that substitutes, omits, or
  replays an HPKE ciphertext or keyring cannot make an authorized recipient
  accept the substituted key.
- The cutover transaction rejects later uploads under the old epoch and any
  upload from a device no longer active at the current state. Batches committed
  before it remain historical input, including writes from a device
  subsequently removed. A remaining offline device retains pending
  operations, fetches and verifies the new state and grant, checks each
  pending operation's ID against committed results, then re-encrypts only
  uncommitted operations in a new batch without changing their IDs or HLCs.
  If the old upload response was lost, pulling its committed IDs prevents a
  duplicate; a crash resumes from the durable local outbox. A removed device
  cannot publish pending work to the original
  Family, but keeps it locally and may explicitly copy it into a fresh one.
  A malicious relay hiding the rotation from a stale writer still has the
  documented old-key disclosure limit; this candidate does not claim to
  prevent it. A deliberately malicious authorized manager can still destroy
  shared availability by issuing destructive data operations or a malformed
  rotation; the no-owner/no-quorum product does not protect against hostile
  co-managers. Other clients must reject unverifiable keys, preserve their
  local data, and report the Family blocked rather than silently discarding
  history. Preventing that manager attack would require a different product
  authority decision.

### Direct invitation and private copy

- A manager creates a signed, single-use invitation naming Family, invitation
  ID, exact role, invitation public key, issuance head, and bootstrap
  descriptor hash. Its private signing key is the bootstrap secret in the
  link fragment; the authenticated descriptor carries the non-secret anchors
  needed above. The link contains no Family key or offline-decryptable key
  envelope. Before contacting the relay, the new device durably creates its
  own Family-scoped keys and enrollment nonce. It signs the exact redemption
  together with the invitation key. The relay atomically consumes the
  invitation and commits that device and fixed role as **pending key
  activation**. Previewing does not consume it. A different device cannot
  take over a consumed invitation, and only the enrolled device credential,
  not the link alone, can authenticate a result lookup after a lost response.
  The pending enrollment is the one successful use even if data delivery is
  still outstanding.
- Single-use is enforced within one non-forked control chain. A malicious
  relay can present sibling chains in which the same invitation was consumed
  differently, just as it can fork manager removals; this is part of the
  already accepted lack of global agreement against a malicious relay, not a
  claim that two redemptions can commit on the honest relay's one current
  head.
- **Only after that commit**, any active authorized device holding the current
  epoch key may automatically create the pending device's grant without a
  second human approval. Its signed grant transcript binds Family and current
  control head, epoch and key commitment, enrollment and invitation IDs,
  fixed role, sender device, recipient device and agreement-key version, HPKE
  suite/context, a random activation-token commitment, and ciphertext hash.
  The relay first commits the grant while the enrollment remains pending. The
  newcomer verifies that transcript, decrypts the grant, keyring, and token,
  checks every epoch commitment, and returns the token in a request signed by
  its enrolled device key. Activation is a second ordered compare-and-swap:
  the relay rechecks that sender and recipient are current, the grant and
  enrollment are still pending, and the epoch has not changed, then marks
  that exact device active. Revealing the one-time token does not reveal a
  data key; recipient request authentication prevents the granting holder
  from activating a device it does not control. Only then does the client
  enable shared reads, writes, or membership actions. Thus an invited manager
  cannot exercise manager authority while still keyless. A stale grant is
  retried against the new head/epoch; removal of its recipient or an epoch
  change leaves that grant unusable. The separate effect of removing the
  invitation issuer remains part of D8.
- If no holder is online, the UI says **joined, waiting for a Family key** and
  disables all shared actions until activation; it does not claim the data is
  available. Each step persists its state and idempotent retry identity. The
  relay retains pending enrollment, grant, and activation results so the
  recipient and key holder may connect at different times. The apps try to
  advance the next eligible step during ordinary sync and on empty wake
  pushes, including when platform background execution is available. A user
  need not deliberately reopen either app between normally delivered wakes.
  On a delayed or unavailable wake, the status stays pending and sync resumes
  on the next opportunity; there is no guaranteed completion deadline. The
  visible stages distinguish enrollment pending, waiting for a key holder,
  verifying the grant/loading history, and ready. Availability can still
  delay the join indefinitely and cannot be solved by the relay because it
  lacks the key. FS51 owns this accepted UX; FS47 still tests the proposed
  activation mechanism. Expiry, cancellation, and issuer-removal races at
  enrollment remain D8 and must be settled before this design is accepted.
  An unintended first bearer remains the accepted D2 risk.
- A private copy uses a local transaction: retain the original Family and
  its outbox, construct a new local-only Family with fresh Family/device IDs
  and keys from locally held data operations **including pending work**, and
  publish the new Family only when its log and projection commit together.
  Membership, relay, device, grant, invitation, and recovery credentials are
  never copied. Replayed source operations receive new IDs and the copying
  device's new-Family authorship; provenance remains non-authoritative local
  metadata. Copying cannot invent records a stale device never received, and
  it never moves another caregiver or silently redirects an action targeting
  the original Family. A still-authorized holder may also make this copy.
  Exact conflict-history retention and stale quick-log presentation remain
  D6-D7.

### Local-to-shared promotion

- A local Family starts with a random globally unique Family ID, per-Family
  device keys, and stable record and operation IDs, all of which remain when
  it becomes shared. Promotion does not rewrite the live local log or turn it
  into a second Family. In one local transaction the client records a
  promotion ID, a log watermark, the signed genesis/epoch commitment, and a
  manifest of encrypted immutable history chunks through that watermark.
  New local operations continue normally above the watermark in the durable
  outbox.
- The relay creates a non-joinable staging Family keyed by that promotion ID.
  Chunk upload is idempotent and each hash, count, and covered operation-ID
  range is bound by the signed promotion manifest. Only after every manifest
  object is present does one compare-and-swap activate genesis and its base
  history. Invitations and ordinary sync are unavailable while staged. The
  client verifies an activation result by promotion ID and committed manifest
  hash, then atomically records shared state locally; operations above the
  watermark upload through the ordinary outbox. A crash or lost response
  resumes the same staging record. A definite pre-activation failure leaves
  the local Family authoritative and shareable by a new attempt; an unknown
  result remains pending until queried and is never shown as confirmed.
- A joining client verifies the promotion manifest and obtains every declared
  chunk before calling the initial history complete. A malicious relay may
  withhold a chunk or an entire valid fork, as already accepted, but cannot
  substitute a different chunk or make an incomplete manifest verify. Exact
  chunk sizing and whether the manifest uses a flat list or tree are wire
  choices, not new product decisions.

### Recovery boundary left open

D4-D5 still decide what a person, device, and recovery holder mean. This
candidate therefore does not place a recovery phrase or platform backup in
the active authorization set, let it sign requests, or infer its former role.
It may decrypt an addressed recovery envelope, but enrolling a replacement
device and disposing of old devices/credentials must follow the future D4-D5
policy. Two implementable alternatives remain: require an active manager to
authorize the replacement (strong revocation, but no original-Family recovery
for a lost sole manager), or make a recovery credential a bearer capability
for a specified role (sole-manager recovery, but theft or an unrevoked old
copy can restore that authority). File restore into a new local Family remains
available independently under D3.

### Multiple-Family isolation

- Every durable log, projection, record reference, outbox item, control head,
  grant, recovery object, sync cursor, and pending transition is keyed by its
  Family ID. Storage transactions and typed core handles reject a reference or
  key from another Family before encryption or projection. Each Family has
  independent device signing/agreement keys and epoch keys; there is no global
  signing identity or fallback key lookup.
- Every signed or encrypted protocol transcript includes Family ID and relay
  endpoint identity in its domain-separated context. The invitation fragment
  selects and authenticates that exact pair without changing the app's active
  Family or any existing Family. Relay routing, push wakeups, export/backup,
  removal, retry, and private-copy state operate on an explicit Family handle.
  Removing or blocking one Family stops only its own outbox and authority;
  another Family's cursor, timers, keys, and pending work remain unchanged.
- Exact database constraints, backup container paths, and cross-Family
  negative vectors still belong in the wire/storage contract. Network and
  push metadata can correlate Families on one device as the threat model
  already states; cryptographic isolation is not anonymity.

This proposal needs negative vectors for grant/keyring substitution and
replay, cross-Family/cross-holder grants, bootstrap-anchor substitution,
keyless-manager actions, manager races and crash boundaries, forged batch
device IDs, accepted-before-cutover/lost-response rebatching, role or device
substitution in invitations, no-holder-online delivery, staged-promotion
interruption/incompleteness, recovery-holder non-authority, multi-Family
storage scoping, and private-copy interruption. The symbolic FS cases cover
user-visible outcomes; exact byte contracts and cross-language vectors follow
only after review.

## Open questions

1. **Role identity and recovery.** Any manager may invite, remove, or change
   another manager's role as long as one manager remains. Decide whether a
   signed role grant applies to a person or an individual device and how
   manager authority is recovered after losing a device (D4-D5). A manager
   acting maliciously can still win a rotation race; the design accepts this
   limit.
2. **Single-use direct join.** D2 decides direct join without a second
   manual approval and accepts asynchronous automatic handoff with visible
   pending stages. Review the candidate consumption, device-bound resumption,
   role binding, and key delivery without a reusable raw Family key in the
   link. Define and test each platform's background sync and delayed-wake
   fallback without promising guaranteed wake delivery. Resolve D8 expiry,
   cancellation, and issuer-removal races. Test concurrent redemption,
   previewing, retries, and unintended-first-recipient cases.
3. **Encoding.** Confirm CBOR over protobuf once the core exists. The
   requirement is verbatim preservation of unknown fields.
4. **Web key storage.** The web client keeps its holder keys in IndexedDB.
   Decide whether to require a passphrase to unlock them on shared
   computers.
5. **Batch authorship.** Review the candidate signed-batch envelope,
   operation identity, unknown-result rule, and device-sequence allocation
   above; specify exact canonical bytes, rejection receipts, and authorization
   check at the committed log position.
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
10. **Local-to-shared promotion and fork.** Review the stable-ID, staged
    manifest, watermark, and activation boundary above for **all** existing
    history, which U2 already requires on first share.
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
13. **Multiple-Family isolation.** Review the per-Family keys, storage scope,
    explicit handles, endpoint bootstrap, and failure isolation above. Specify
    exact database and backup partitioning and negative vectors. Separate keys
    do not prevent server correlation through network or push metadata.

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
