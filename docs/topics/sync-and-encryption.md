# Sync and encryption

Status: M-1 design decided, September 2026; M0 has started with Rust
canonical CBOR, structural operation decoding, and first crypto byte vectors.
Most implementation and executed validation remain. The normative v1 byte
and state-machine rules are in
[records](../protocol/records-v1.md),
[sharing](../protocol/sharing-v1.md), and
[portable files](../protocol/portable-file-v1.md). Earlier candidate
mechanisms below are rationale and yield to those contracts wherever wording
differs.
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
- After a removal commits, the removed device receives no new epoch key and
  cannot read entries encrypted under later epochs. Data already on their
  device remains. An honest relay rejects old-epoch uploads after cutover;
  a malicious relay hiding the change from a stale writer can expose that
  writer's old-key ciphertext to a removed holder of that key.
- A lost device cannot restore its old Family authorization from a recovery
  credential. A still-active manager can invite a replacement device. A
  locally held copy or complete file backup can instead establish a new
  independent Family with the records it actually contains; loss of every
  copy and backup can mean permanent data loss.
- Clients on different app versions interoperate without dropping data.
- One app can participate in several independent Families without a global
  account; a removal or sync failure in one does not affect the others.

## Operation log and merge

The exact rule is in [records v1](../protocol/records-v1.md).

- The unit of change is an **operation** on one record: `create` (id, type,
  fields), `set` (id, field, value), or `delete` (id). Every operation
  carries a hybrid logical clock (HLC) stamp: wall-clock milliseconds, a
  counter, and the writing device's id. HLC is informational and cannot
  dominate field values forever when a writer supplies a hostile timestamp.
- **Merge** is last-committed-writer-wins per field by verified relay cursor
  and within-batch index; local-only history uses its append index. This
  converges on a common relay history but displays
  only one of two concurrent values for the same field; the operation log
  retains both. `delete` sets a tombstone
  field, and an explicit restore operation can clear it. A `set` requires
  its create earlier in verified log order or earlier in the same batch;
  an unknown record target makes that signed batch inert for all replicas.
- Operations are grouped into signed, encrypted **batches** for upload. Each
  has a client-generated ID and a public per-device accepted sequence bound
  by the signature; clients detect gaps in a history they receive. A relay
  that hides a suffix may still conceal the latest entry.
- The append-only log is the source of truth. A rebuildable projection holds
  the current state of each record, including the winning stamp per field
  and tombstones. The UI queries that projection rather than replaying the
  log for every screen. Native SQLite writes an accepted operation and its
  projection change in one transaction; web IndexedDB uses an equivalent
  transaction. The projection can be rebuilt by replaying the log. No
  log-compaction or snapshot protocol is required in M0; measure replay
  before adding snapshots.

## Keys

Version-1 key choices follow the normative sharing contract.

- **Epoch keys.** Batches are encrypted with a random 256-bit epoch key using
  XChaCha20-Poly1305. The associated data binds the family id, epoch number,
  protocol version, batch id, and device id, so a ciphertext cannot be
  replayed into another family or epoch.
- **Key holders.** Authorized Family-specific devices hold grants to epoch
  keys. Each has its own signing and agreement keypairs and acts only under
  its own recorded role. The MVP has no recovery-phrase or platform-backup
  key holder and no credential that can reinstate old membership. A new
  installation obtains authority only through a fresh device invitation.
- **Membership** has a signed public authorization transition with a
  matching encrypted membership object. A device joined (with its public
  keys and role), changed role, or was removed. Readable caregiver and device
  labels stay encrypted and are not authority claims. Clients verify the
  public transition and exact decrypted match. The two product roles are manager
  (read/write plus membership management) and member (read/write only).
- **Grants.** A new epoch key is wrapped for each current holder with HPKE
  (RFC 9180, X25519, HKDF-SHA256, ChaCha20-Poly1305) and stored on the server
  addressed by an opaque holder id.
- **Keyring.** Each rotation also stores the list of all earlier epoch keys
  encrypted under the new one, so a newly joined holder can read the full
  history while a removed one gains nothing.
- **Server auth.** Each device signs requests with its Family-specific key.
  The relay enforces the public authorization state and receives no data key.
  A key derived from a shared epoch key cannot enforce manager-only actions
  because every reader knows that key.

During enrollment, invitation and pending devices can read signed public
controls but not encrypted Family batches. They verify control ancestry
across any skipped batch cursors and persist that pending progress. After
admission, the new holder reads and verifies the contiguous full log and
required data objects before the Family becomes ready. A filtered-control
cursor alone never establishes complete shared history.
Signed read bytes can travel in a GET body or an Authorization header; the
relay verifies the same canonical request and rejects ambiguous transport.
The header lets Android fetch pages with its standard HTTP client. The
[protocol](../protocol/sharing-v1.md#relay-routes-and-response-bytes) defines
its exact syntax.
Recipient enrollment stores either an ordinary invitation immediately after
genesis or a sparse control ancestry when data batches precede it. The
nonempty pre-admission batch path is unsupported; the sparse path persists
the claim, shared root, and issue ancestry together before contacting the
relay, then hydrates the full log after admission.
An enrollment attempt does not appear in the local-only tracker Family list,
and the local-only API rejects activity or backup access through its handle.
Its later shared data view must come from the verified shared-session API.
Prepared holder challenges are reconstructed from the verified historical
claim prefix on retry, including after later controls commit. The holder
chooses the recipient from verified pending authority state in Rust.
The recipient's proof also rebuilds from its verified challenge prefix after
a later control commits. A pending sparse-control cursor remains distinct
from verified full-history readiness throughout this exchange.
The first holder admission is confirmed only after the saved challenge proof
is verified against the holder's verifier object. A committed grant alone
does not make the recipient's data view ready.

Per-holder keypairs are needed because a removed device that knows the old
shared key could otherwise read the new key while it is handed to the
others. Roles limit use of the app's shared family; they cannot stop a
member who can decrypt all data from copying or exporting that data.

Durable recipient enrollment secrets and epoch keys use a local 32-byte
wrapping key supplied through a platform adapter. The Android development
adapter now protects that key with Android Keystore; the core encrypts Family-scoped
secret rows before SQLite commit and never sends the wrapping key to the
relay. This local key is not a recovery path: losing the installation and
its key still requires an existing manager to invite a new device or a
separate saved file to create a new Family.

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
from correlating Families through IP addresses or push tokens. V1 uses
Family-scoped keys and composite local storage identity; files omit keys.

- **Local-only by default.** A new family logs through the same core model
  without contacting the relay. The user explicitly chooses to share it.
  First launch may instead join an invitation without creating an empty
  local Family. The user may create further local Families later.
  The first share includes its existing history, retaining its Family and
  operation IDs through atomic promotion.
- **Shared.** A manager enrolls another device as a manager or member. Both
  roles can read, log, edit, and export the locally held family data. Only a
  manager can change membership. Any manager may remove or demote another
  manager, but a shared family must retain at least one manager. There is no
  owner hierarchy or approval quorum. An ordinary member still has enough
  data to create an independent copy outside the shared family.
  Competing removals use the first valid relay-committed change; a stale
  request cannot exercise revoked authority. An offline request or missing
  response must not be presented as a completed change. Either manager may
  explicitly make an independent copy; the removed device gets one
  automatically when pending local work needs a destination. Unaffected
  members stay in the original Family. See product D1/D7 and FS07-FS12.
- **Removed or departed.** Once the client learns that its device membership
  has ended, it clearly says that sync for this family has stopped. It does not
  erase local history. With pending work or a new action targeted at the
  removed Family, the client creates or reuses one private Family copy with
  fresh identity and keys. Otherwise it offers an explicit copy and keeps
  locally held history available. That copy starts local-only and can be
  shared later; it does not silently rejoin or merge with the original.

## Invites, removal, and recovery

- **Invites.** A manager authorizes one enrollment and its role when creating
  the invitation. A QR code or remote link carries single-use bootstrap
  access; remote secrets belong in the URL fragment. No second manual
  manager approval is required. The previous raw epoch-key payload is
  superseded: it must not allow link holders to bypass consumption and read
  history independently of enrollment. The keyless claim, holder challenge,
  proof, and atomic admission/grant are fixed in sharing v1.
  The joining device generates its own keys; the relay atomically consumes
  the invitation with enrollment. Concurrent redemptions cannot create two
  enrollments. Previews do not consume it, and failed/lost responses allow
  authenticated resumption only by the same enrolled device. A fresh device
  needs a fresh invitation. Product decision D2 accepts that an unintended
  bearer may redeem a valid unused invitation first.
- **Invite lifecycle.** D8 sets seven days for unused redemption, cancellation
  by any active manager, and invalidation of unused invitations when the
  issuer loses manager authority. A committed enrollment remains pending
  through later expiry or issuer removal; an active manager may separately
  remove that pending device. The wire contract must enforce these outcomes
  against the ordered control head using relay-signed commit time. A
  malicious relay can extend real-time expiry by lying about that time.
- **Removal.** A manager can remove a holder. The removing device creates
  epoch e+1, grants it to every remaining holder, posts the keyring and the
  new authorization state, and appends a signed removal transition. The
  relay rejects subsequent requests from the removed holder, while the
  removed device can read the opaque signed chain to verify its removal.
- **Replacement after loss.** An old grant, file, or account cannot authorize
  a fresh installation that lacks the enrolled device's private signing key.
  A copied private key may impersonate that same device; revoking its device
  credential revokes all copies. If another manager device remains active,
  it can invite a fresh device with an explicit role. If no manager can act,
  original-Family authority cannot be restored through the MVP. Other member
  devices may keep reading and writing, but cannot revoke the lost manager
  key or invite a replacement. A local copy or saved full file can seed a
  new independent Family. Relay ciphertext alone is not a usable backup for
  a fresh device.
- **File backup and independent restore.** Product D3 requires a complete
  restorable data file with optional password protection. This applies to
  local-only and shared Families. Restore creates a new local-only Family
  with fresh identity and keys, saved records, and no original membership or
  device credentials. It works without the original relay or manager. The
  person can share the new Family and reinvite caregivers; recreating the
  old shared group is not required for this path. No key or phrase without a
  record copy can restore missing records. The event topic specifies the
  file contract.

## Server API

A relay with no knowledge of event content. Endpoint routing remains M0
implementation work; the signed objects, ACLs, and atomic results are fixed
in [sharing v1](../protocol/sharing-v1.md):

- register a local family for sharing (family-id rule and initial manager
  authorization fixed in M-1);
- stage idempotent encrypted promotion chunks and atomically activate their
  signed manifest;
- append a signed batch (idempotent by batch ID; server assigns a global
  cursor and verifies the device's accepted sequence);
- list batches after a sequence number;
- put and get grants and the keyring for an epoch;
- commit invitation creation/redemption, grant activation, and epoch rotation
  with opaque membership authorization atomically;
- a WebSocket for live notification of new batches;
- later, register a push token and send empty wake pushes on new batches.

The future push sender is an interface with APNs and FCM implementations
first and UnifiedPush later (see the plan's service interface requirements). Quotas
limit storage per family and requests per family and IP.

### Transport and wake behavior

The verified ordered log and signed receipts are the durable sync source.
Clients fetch after their last verified cursor on startup, reconnect,
foreground return, and a background wake. An online client may keep a
WebSocket open for low-latency notice of new entries. The notice is only a
wake hint: the client still fetches, verifies, decrypts, and projects the
log. If the socket is unavailable while the client can run, bounded
polling with backoff provides another wake path. No persistent connection
or simultaneous device availability is required for eventual progress when
clients later reconnect to an honest relay.

M0 proves this with a real relay and separately stored CLI/browser clients,
including disconnected clients, lost notices and responses, and restarts.
The first Android sharing flow uses foreground polling and scheduled
background work. FCM and APNs may later provide empty background wakes through
app-owned interfaces to reduce latency when an app is suspended. A missed
push leaves the durable attempt pending for the next available run. A wake
never proves a batch, admission, or removal
committed. The client obtains signed acceptance evidence or a verified log
entry before showing shared success. The UI exposes each pending stage.

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
does not hold. Per-device Ed25519 batch signatures are required; knowing
the shared epoch key does not authorize impersonating another device.
The byte vectors and mixed-client relay tests must verify that rule.

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

The Rust core executes CB01-CB06 and UNIT01-UNIT03 from
`tests/vectors/records-v1.json`, the fixed signed encrypted batch from
`full-wire-v1.json`, and the encrypted minor/inert batch cases from
`negative-batch-v1.json` with `cargo test -p babytrack-core`. This covers
canonical CBOR, operation and published field checks, SHA-256/Ed25519/
XChaCha known answers, fixed HPKE decryption, and in-memory atomic batch
replay. A signed encrypted multi-batch test covers cursor winners, extreme
HLC, tombstone/restore, duplicate IDs, and rebuild equality. Exact HPKE
sender bytes, durable replay, remaining byte vectors,
cross-language agreement, and the protocol/security gates below remain open.

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
- Device-loss tests: a new installation without the enrolled private key
  cannot claim a lost device's role using its old grant, a file, or an
  unrelated account. A copied device key acts as that same device until its
  credential is revoked. A remaining manager
  may invite it explicitly; if none remains, original-Family access is not
  restored. Removing one device does not silently remove another device
  held by the same person.
- File loss-recovery tests: readable and protected backups restore saved
  data into a new independent Family without original shared access. An old
  backup never reinstates revoked membership. Also verify that no-backup
  states do not claim lost records are recoverable.
- Sync torture test with a real server and many simulated clients going
  offline and reconnecting.
- Plaintext marker test: known strings written into events never appear in
  the server database or logs.
- Run the agreed [sharing scenarios](../scenarios/family-sharing-scenarios.json)
  with both race orders and lost-response/restart cases. All FS cases are
  agreed specifications; M0 binds them to executable actions and assertions.

## V1 design decisions and remaining validation

The [sharing protocol](../protocol/sharing-v1.md) is the exact owner of
control transitions, keyless invitation claim, holder challenge/proof,
atomic admission/grant, epoch rotation, signed batches and receipts,
verified removal, promotion, and Family isolation. The
[records protocol](../protocol/records-v1.md) owns canonical CBOR, operation
identity, field winners by verified log position, invalid-batch handling,
and version skew. The [portable file](../protocol/portable-file-v1.md) owns
readable and protected backups. These replace the earlier candidate
transcripts and prevent two competing wire descriptions.

A malicious relay can withhold or fork a valid control history, lie about
its signed clock, and deny service. Signed receipts and device sequences
make observed tampering detectable but do not prove global non-equivocation
or latest-entry inclusion. Expiry and first-committed conflict outcomes are
honest-relay guarantees. An authorized holder can commit an unusable grant;
that delays recipient readiness and is repaired by another active holder or
rotating removal. An authorized writer can submit malformed encrypted data;
clients treat that data entry as inert, report it, and continue replay.

M0 still validates platform key storage, background wake and delayed wake,
real relay crash atomicity, readable/protected export, and cross-language
vector agreement. A browser on a shared computer may expose its local
Family keys to anyone with that browser profile; an optional local app lock
is M2 product work and is not an original-Family recovery credential.

### Proposed implementation seam for general relay authority

The client control chain replays all published transition kinds. The relay
now uses the shared public verifier for the first cohort and later control
routes, with later-device negative and hydration coverage still in progress.
The implementation seam factors
the deterministic **public** authority transition rules into `wire/`, which
already supplies canonical CBOR and public cryptography to both sides. The
candidate phase consumes the previous public state, a durable public history
ledger, and signed candidate bytes. It checks canonical bytes, version,
Family/relay ID, parent head, roles and signers, deterministic state effects,
state hash, manifest requirements, epoch, ID reuse, and transition-specific
invariants. It returns a prepared next state and object requirements, with
no receipt, cursor, time, or data-readiness conclusion. The ledger retains
historical device credentials and lifecycle status, issue commit times,
head-to-epoch mappings, exactly one verified commitment per epoch, accepted
per-device sequences, public IDs, permanent Family-wide staged object
ID-to-hash reservations, and admission/grant associations. The canonical
seven-field authorization state
alone cannot answer historical reads or batch ancestry. The shared verifier
never accepts an epoch key, decrypts an object, or interprets event data.
The exact [per-kind manifest contract](../protocol/sharing-v1.md#authorization-state-and-ordered-log)
allows empty manifests for cancellation, role change, and pending removal;
their manager signatures authenticate their complete public state effect.
An uncommitted staged object's permanent ID binding blocks another public
category from claiming that ID. Its own exact kind/hash binding remains
usable by the staging candidate; already committed objects retain their
matching reservation without counting as a second ID birth.

The client continues to verify relay receipts, pin the committed head,
open addressed grants and membership objects, and project encrypted data.
The first-cohort relay checks admission grant public envelope bindings at
stage and commit, while only the client can open its ciphertext and confirm
the epoch key; a signed admission may still need grant repair if that
ciphertext is unusable.
The relay resolves exact committed/rejected retries before checking current
authority. Candidate-scoped staging permits competing proposals at one head;
staging grants no authority. Inside one SQLite write transaction it reloads
the public ledger, reruns candidate verification, chooses and checks the
actual commit time (including invitation expiry), checks staged hashes,
sizes, and public envelope bindings, then atomically commits the signed
receipt, global cursor, control head, public ledger, ID reservations, object
visibility, and exact result. Controls and batches share this cursor CAS;
head-only CAS is insufficient when a batch advances the cursor. Path-specific
GET access uses historical credentials/status from the ledger: a removed
device may prove removal and query its own earlier result but cannot read
new data, grants, or another author's result. Rejected-only batch IDs remain
a separate durable relay reservation. The ledger is rebuilt from authenticated
signed controls and receipts or verified against that chain after restart,
not trusted as an unchecked database snapshot. Each committed head binds one
epoch; each epoch binds exactly one commitment from verified transition bytes.
The shared `EpochBindings` type enforces these immutable associations. Client
control replay and the first-cohort relay use it; the relay reconstructs the
bindings on restart and checks each accepted batch against its historical
active signing key, exact signature, next per-device sequence, globally new
batch ID, authoring-head ancestry, and active epoch. General route commit
transactions now use the same verified history.
Damaged or incomplete reconstruction refuses further commits and reads.
On reopen, each extant rejected-result row requires a valid relay signature,
exact envelope hash/ID/sequence/Family/relay binding, and a signed cursor/head
present in the authenticated historical log. A relay-private signed checkpoint
also binds the complete sorted sets of permanent staged-object reservations
and rejected results. Staging and rejection update it in the same SQLite
transaction. Missing or modified rows fail startup; a saved older matching
checkpoint and rows can still be replayed by a storage attacker because v1
has no external monotonic witness.

Nonempty development relay databases created before this checkpoint cannot
start normally. The operator may explicitly rebaseline one after checking its
existing data with
`babytrack-server migrate-private-checkpoint DB_PATH SEED_FILE`. This
migration verifies extant public history and rejected rows, then signs the
private set it finds. It cannot establish that a rejected-only row or
reservation was not lost before migration. No Family data or relay seed is
sent elsewhere by this command.
An offline request or staged object never becomes a
confirmed authority change. Unknown versions/kinds fail closed.

The current first-cohort relay authenticates its saved genesis candidate and
receipt against the cursor-one log entry before authority reads or commits.
On restart it also checks contiguous cursor positions, control receipt
signatures and parent links, committed object bytes against signed manifests,
the saved head and cursor, and accepted batch receipts and metadata against
their log bytes. The relay now also replays public control semantics with the
shared reducers on restart. Full batch author semantics, rejected-result rows,
and other derived-row reconstruction are still required for the general ledger.

The extracted `wire` verifier prepares invitation-issue, two-signature
claim, holder-challenge, pending key-proof, admission, and active-removal
candidates for both
the client control chain and relay. It checks the
public state effects, hashes, manifests, and applicable signatures without
time, receipt, or object opening. Historical ID checks still live in the
callers. Shared reducers now also derive cancellation, role change, pending
removal, and grant repair effects and signer policies, while a common signed
envelope check enforces their bytes and signatures. The relay has no general
routes for those four kinds yet.
The no-object manager transitions (unused-invite cancellation, role change,
and pending-device removal) now have a transactional relay commit primitive
that reconstructs the signed public ledger inside the SQLite writer lock,
checks protocol IDs and actual commit time, and returns the original signed
result on an exact retry before checking current authority. It is not exposed
over HTTP until general batch write authority and later-device route tests
use that ledger.
A regression found and corrected an erroneous manifest requirement in the
shared reducer and client replay: these three transitions have no objects in
v1.
Authenticated relay GETs now classify readers from a freshly reconstructed
public ledger. Active managers and members use their current signing keys;
unused invitation credentials can read their one issue object; pending
devices can read their latest addressed challenge object; historically
admitted but removed devices retain only their own signed-result access.
The invitation-result route finds the matching committed issue by ID rather
than assuming a fixed control position. First-cohort vector replay asserts
each of these reader stages. Full later-device route tests and a more
efficient verified ledger cache remain before general authority exposure.
Repair preparation requires a verified admission association and current
epoch commitment from the caller's historical ledger.
The common envelope verifier alone cannot authorize these controls. The
public ledger and general relay authority are not
complete. The [focused verifier advisory](../tactical/003-m0-foundation.md#advisory-shared-authority-verifier-at-7566262)
records this requirement and staging/restart hardening findings.

This is an implementation proposal, not a new wire contract. Its first
regression is byte-identical replay of the reviewed first cohort on both
sides, including removal and signed post-removal result lookup. Then add a
third device, manager-to-manager changes, unused-invite cancellation,
pending removal, role changes, and general rotation, with route-level
negative and crash/restart cases. Review the factoring and its trust split
before replacing the relay's current first-cohort checks. The focused
[Daybreak advisory](../tactical/003-m0-foundation.md#advisory-general-authority-seam-review-at-bb68156)
found the earlier one-phase/current-state sketch unsafe; these transaction,
history, and retry requirements are its required corrections. The
[M0 tactical](../tactical/003-m0-foundation.md) tracks delivery.

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
