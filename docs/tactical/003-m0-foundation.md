# 003: M0 executable foundation

Status: in progress after completed [001 M-1 design closure](001-pre-m0-design.md).
This workstream
turns the [versioned contracts](../protocol/README.md) and
[scenario catalog](../scenarios/README.md) into shared core, relay, and CLI
behavior. The [MVP plan](../mvp-plan.md#milestones) owns scope.

## Goal and exclusions

Deliver one interoperable encrypted Family through Rust, Swift, Kotlin, and
wasm; durable local records and offline outboxes; a real encrypted relay;
verified joining, removal, private copy, and backup restore. Use executable
fixtures with injected clocks and network delivery. M0 has no product UI,
hosting, publishing, store work, clinical content, or vendor-service logic.

## Ordered slices and gates

### 1. Bytes and bindings before core API freeze

- [ ] Implement strict deterministic CBOR and version checks. Run every
  `tests/vectors/` byte fixture in Rust, Swift, Kotlin, and wasm. Reject
  noncanonical and cross-Family bytes, preserve opaque minor fields, and
  compare actual hex to expected hex.
- [ ] Encrypt, sign, decrypt, and project one fixed child event through each
  binding in both directions before freezing the core API. Make wrong key,
  wrong Family, wrong AAD, and wrong signature fail in every language.
- [x] Run a minimal real browser harness using wasm and IndexedDB. Verify
  reload, transaction rollback, and key isolation with no product web UI.

Gate: the same encrypted event and negative vectors pass independently in
four runtimes; no platform adapter reimplements CBOR, crypto, or merge.

### 2. Durable local core

- [ ] Add the operation log, field projection, tombstones, raw unknown-field
  retention, per-Family HLC metadata, and local outbox in Rust. Native
  SQLite and browser IndexedDB atomically append/project/dedupe. Rebuild
  equals incremental projection after interruption.
- [ ] Implement local creation, child/activity scopes, explicit Family/child
  handles, timer target capture, and independent private copy with one
  idempotency key. Exercise FS01-FS02, FS13-FS18, FS22-FS25, FS33,
  FS48-FS49, FS54, FS58, FS60-FS61.
- [ ] Implement readable and protected portable files through the core.
  Restore to a fresh Family and verify saved point, opaque fields, bad
  password/corruption, and crash before commit (FS26-FS30, FS40-FS45).

Native shared editing now validates against the verified relay projection
plus every unsent local operation, so a recipient can edit a manager-created
child. Shared readable export includes that pending work and records its
verified source cursor; local-only export rejects a shared Family. An
explicit private copy restores the snapshot into a fresh local Family and
records the source-to-copy mapping in the restore transaction, so repeated
requests after restart reuse one destination. The dynamic two-device relay
test exercises these paths. Automatic copy on verified removal, timer
targeting, browser parity, and adversarial crash coverage remain open.
The native production binding now exposes offline Family creation, explicit
Family/child targeting, child lists, bottle and diaper entries, timelines,
and readable backup/restore through the shared Rust core. The Family
metadata create and its local row commit atomically. Rust, Kotlin, and
Swift smoke paths create two Families, reject a cross-Family child target,
reopen the database, and restore a backup. The Android UI and broader
activity set remain M1 work.
The native API now also exposes the core's fixed-profile Argon2id protected
file path. Wrong passwords leave the Family list unchanged; a correct one
restores into another local Family. Rust, Kotlin, and Swift round trips pass.
The Android UI offers the password option and uses the device's current
available-memory estimate rather than weakening the KDF on a low-memory
device.
Completed sleep now uses the published start/end instants through the Rust
local and shared APIs. Android records a duration with the start and end
zone offsets and shows it in the timeline. The local backup/restore test
preserves the end instant; the two-emulator relay test checks that the
recipient's sleep entry reaches the manager.
Notes now use the published text field in the Rust local and shared APIs.
Android retains typed note text until the write succeeds. The local
backup/restore test preserves the note, and the two-emulator relay test
checks its delivery to the manager and checks that a distinctive plaintext
marker is absent from relay storage and logs.
Sleep timers now create the published open sleep event and stop it with a
field update against the explicit Family, child, and activity ID. The Rust
core rejects a wrong child, wrong Family, or repeated stop. Local restart
and backup/restore retain the end; in the two-emulator flow the recipient
starts a timer, the manager sees and stops it, and the recipient later sees
the completed interval. FS24's target isolation is covered in core; a
stale widget/watch action after removal (FS25) remains later-surface work.
The production native binding now exposes durable local-Family promotion
preparation and first invitation issue through the shared Rust state machine.
It returns exact staged object and candidate bytes to a platform transport,
and confirms sharing or emits the one-use link only after the core verifies
the committed relay entry. Kotlin and Swift smokes check that preparation
survives an exact retry and a wrong local wrapping key cannot reopen it.
Pending-join UI and background delivery still need to consume this binding.
The Android debug sharing preview now uses that binding with a Keystore-wrapped
installation key and a byte-only HTTP adapter. A manager enters an exact
relay origin and its public key, then stages encrypted promotion objects and
confirms the signed genesis before seeing a share result; first invite issue
likewise yields its fragment only after signed commit. An emulator test
against a local relay with `adb reverse` promoted a child Family, issued an
invite, reopened the app store, and retried both writes. Android CI compiles
the instrumentation test but does not run an emulator. A separate recipient
store now parses the invitation, fetches its public control page using the
Android-compatible signed-read header, verifies the link in Rust, and commits
an exact keyless claim. The emulator test restarts and retries that claim
without an invitation read, since that authority closes when the claim
commits. The UI reports waiting for a key holder. Key handoff, data readiness,
subsequent edit sync, and removal remain open.
The local-only tracker hides this pending recipient Family and rejects writes
through its handle; the emulator test checks that boundary. A verified shared
session will supply the recipient's usable view after handoff and hydration.
The holder can now fetch the new claim with a signed GET, let Rust select the
sole pending device from verified authority state, stage both challenge
objects, and confirm the committed challenge. The emulator test repeats this
after reopening the holder store. The prepared challenge rebuild uses the
verified three-control historical prefix even when the current log already
contains the challenge, so exact retry survives commit and restart. Recipient
proof and admission remain open on Android.
The recipient can now poll signed pending controls, fetch only its addressed
challenge HPKE object, let Rust verify and open it, and commit a durable key
proof. The separate-store emulator test repeats the proof after process
restart without using the invitation credential. Rebuilding a saved proof
uses its verified challenge prefix even after the proof commits; a sparse
recipient still does not claim data readiness. Holder verification, admission,
and full history hydration remain open.
The holder now fetches and verifies the proof control, has Rust recheck the
pending challenge proof against the saved verifier, then stages the encrypted
membership and epoch-key grant and confirms the admission. The emulator test
retries admission, proof, and challenge after later controls commit; all
rebuild from verified historical prefixes. The recipient still has no usable
shared view until it fetches the full log, required objects, and grant.
The Android recipient now has a manual bounded history pass. Rust signs each
exact read path, verifies the contiguous log and referenced objects, and
opens the committed grant only when the complete visible prefix is ready.
Before admission it reports a pending control stage without claiming data
readiness. The real-relay emulator test then downloads the manager's child,
reopens the store, and verifies readiness again. Retrying the exact saved
proof after a later admission now recognizes its already verified sparse
control even when it is no longer the last control. This does not yet expose
a shared tracking view or schedule automatic sync.
The first shared read snapshot now uses the same Rust record summaries as
local tracking. It opens a manager's durable keys or a recipient's verified
admission grant, overlays unsent local operations, and rejects a keyless
pending recipient. The Android join preview shows shared children and entries
after the bounded sync pass. Its real-relay emulator test checks the pending
denial, manager snapshot, recipient child after hydration, and restart. Shared
editing and automatic refresh are still open.
The native shared binding now appends child, diaper, and bottle operations
through the ready Family session, using the same input builders as local
tracking. The Rust store validates each append against verified shared
history plus the durable unsent overlay. The Android preview exposes child
and wet-diaper actions and labels them as awaiting upload. Its real-relay
emulator test checks keyless write denial, wrong-child rejection, local
overlay visibility, and persistence across restart. Batch upload and remote
convergence remain open.
The Android byte transport now uploads exact signed batches from the durable
Rust outbox, then performs an authenticated full-log pull before reporting
acceptance. Both manager and recipient can run a bounded manual sync pass;
uncertain responses leave the saved envelope for exact retry. The manager's
main tracker now reads and writes through the verified shared session after
promotion; the local-only API rejects that Family so it cannot silently
diverge. The real-relay emulator test has the recipient save and upload a
child and diaper, the manager pull them, then the manager save and upload a
child that the recipient pulls. Both stores reopen and retry without duplicate
records. This is manual foreground sync; wake scheduling, removal races,
shared backup UI, and general epoch handling remain open.
The recipient's encrypted durable attempt already holds its verified
invitation origin and keys. Android now lists pending and ready recipient
Families from the Rust store and can resume proof, history loading, and sync
by selected Family after process restart without retaining the fragment in
UI state. The manager's nonsecret relay origin is saved in Android settings
after confirmed promotion; Rust still pins and verifies the relay identity.
The real-relay emulator test reopens the recipient and resumes by Family ID.
The Android upload loop now reads each uncertain batch's signed result before
reposting. Rust binds that result to the exact durable batch. A signed
acceptance ahead of the visible log keeps the batch pending; signed stale
epoch or sequence-conflict rejection clears uncertainty only after the
required verified authority prefix, allowing fresh bytes. Unsupported
rejections retain local work and block upload. The CLI competing-sequence
regression checks denial before the competing accepted prefix, a forged
result, and resealing after verification. The emulator drops one accepted
upload response and recovers by pulling its signed log entry. Removal/private
copy UI for unsupported rejections remains open.
The first-cohort Android coordinator now asks Rust which handoff action is
next after replaying verified controls. Foreground polling every 30 seconds
advances challenge, proof, admission, and bounded history sync when the
required device and relay are available; no concurrent app session or second
manual approval is needed. An emulator test alternates manager and recipient
processes through the full join. This is an app-open wake path only. Scheduled
suspended-app work, push, and general later-device enrollment remain open.
The shared Android view now displays durable unsent-operation counts and a
warning when an authorized signed batch was inert during verified replay.
Sync distinguishes a saved local outbox from an uncertain exact batch result.
The real-relay emulator test checks that two offline edits remain visible as
pending after restart and drain after a signed acceptance. The Rust replay
test already covers continuation past an authorized unopenable batch; an
Android injected hostile-batch display test remains open.
The native shared binding now exports a readable or password-protected file
from verified history plus durable unsent work, and creates or reuses one
private local-only Family copy. The Android manager tracker and recipient
view offer these recovery actions. The emulator test checks the file's
source Family and records, correct and wrong backup passwords, independent
restore, copy idempotence, and local-only child visibility. Verified removal
still needs to trigger the copy automatically when pending work exists.
Android now schedules a persisted network-constrained periodic JobScheduler
job when the app opens. It reuses the Rust-backed coordinator for manager and
recipient Families, including pending handoff and outbox work; the UI still
polls every 30 seconds while open. The real-relay emulator test forces the
scheduled job and verifies that a saved manager edit drains without a
foreground UI sync. Android may defer the periodic job (15-minute minimum),
and a force-stop prevents it until the user next opens the app. Push remains
a later wake optimization; cursor verification remains the sync source.
When Rust verifies an unsupported rejection for an exact saved batch,
Android now shows an explicit blocked-upload message in foreground and manual
sync paths. The edits remain in the durable outbox, and the UI points to the
available private-copy action. Recovery after a verified removal still needs
the automatic copy path; this message does not claim the relay accepted work.
The Android real-relay instrumentation suite now has a CI emulator job. It
uses the APKs built by the Android compile job, starts a disposable relay,
forces the scheduled job, and fails if instrumentation does not report tests
passing. The runner script passed locally against an existing relay, and
`actionlint` accepts the workflow. A remote Actions run has not yet been
observed; it must be checked before treating this new required job as proven.
The advisory follow-up found two narrower restart failures. Stale-epoch
rejection now reconstructs and verifies the control prefix named by its
signed receipt even when later accepted entries have advanced the current
pin; one test accepts a later epoch-two batch before resolving the old
batch, then checks a fresh ID/nonce/sequence across restart (FS48/BATCH04).
A recipient with only a durable prepared claim now retries those exact bytes
by Family ID before its first pending-device read; the Android emulator
tests both a pre-commit failure and a committed claim with lost response
after coordinator restart (FS38/FS51). No wire version change was needed.
An Activity recreation test then exposed that composition disposal closed
native stores before an in-flight sync coroutine finished. The main
installation now owns one Rust local store and sharing coordinator for the
app process; Activity recreation reuses them. The test recreates the screen
with a prepared claim, waits for that exact claim to commit through the
foreground pass, and confirms the join screen remains available. The
five-test real-relay emulator suite passes. A selected-recipient stage
assertion is still open because the current scroll view exposes only visible
text to Android accessibility in this test.
The shared view now lists the verified active device credentials and roles
from the Rust public authority chain. It labels this installation separately
and shows the full Family-scoped device ID, without implying a person-level
account. The emulator checks the manager-only list before admission and both
manager/member credentials afterward. The tracker now exposes manager
removal controls and phone-local labels for each verified device. The Rust
core and Android debug UI can now cancel an unused invitation through an
exact saved manager proposal. A local real-relay test confirms a canceled
link gives a verified canceled status, and a lost cancellation response does
not block the next cancellation. A manager can now prepare and confirm an
exact saved role change for an admitted device. Android relay instrumentation
checks promotion, demotion, and denial of a later invitation by the demoted
device. Admitted-manager cancellation, pending-device removal, and broader
membership flows remain open.
Remote emulator runs through `bba928a` exposed concurrent SQLite schema
initialization: independent Android components could both observe version
one and attempt the same invitation-table migration. Initialization and
migration now recheck the version under one immediate writer transaction.
Rust regressions open both a fresh store and a version-one store from eight
threads at once. Local Android relay instrumentation passes with the repair;
remote run `36343776749` at `14eff1e` passed every required job, including
the disposable-relay emulator and UI smoke.
Remote run `36353404176` later exposed a narrower fresh-open race: SQLite
can return `BUSY` immediately while another connection changes journal mode,
even with a busy timeout. The open path now retries that one setup statement
for the bounded timeout; the eight-thread first-open test passed 200 local
repetitions after the repair. A new remote run must confirm the gate.

Gate: no crash loses a committed local operation or publishes a partial
copy/restore; no Family handle reaches another Family's rows, keys, or file.

### 3. Relay authority and early security review

Canonical CBOR and domain-separated hash, signature, and AEAD primitives
now live in a narrow `babytrack-wire` crate. The client core reexports the
same implementation; the relay can depend on it without importing client
storage, plaintext operations, or Family keys. Later authority kinds and
batch routes below remain open.
The relay crate now encodes candidate-to-committed control receipts, object
stage responses, and ordered control pages from the narrow wire crate.
Exact genesis and invite-issue API byte vectors pass, including the relay
signature. The constructors are internal to validated SQLite transactions.
The relay's genesis candidate validator now independently checks the pinned
relay identity, zero parent, initial manager row and signature, resulting
state/core hashes, epoch, and ordered promotion manifest. It accepts the
published candidate and rejects a wrong relay key or altered signature.
Later control kinds still need public relay authorization and state
transition checks.
SQLite now durably reserves one exact genesis candidate for a Family before
membership exists. Only objects matching its signed manifest can stage;
none is readable as committed data until every object and the signed
genesis receipt land in one transaction. Identical staging/commit retries
return the first bytes. The genesis API fixture passes through staging,
restart, commit, another retry, and reopen; changing the relay signing key
on reopen fails. Only the first invitation transition can follow genesis;
batch writes remain closed.
The first Axum development routes now expose genesis object staging,
genesis commit, and a signed manager-only promotion-result read. Routes
require canonical lowercase path IDs, exact path/body Family and object
binding, canonical CBOR media type, and an exact signed GET path. A router
test sends the published promotion bytes through HTTP, checks pending and
committed results, and rejects a cross-Family path and bad read signature.
The dev binary takes a SQLite path, an existing raw 32-byte relay seed file,
and a bind address. Further invitation/join transitions, batch, and
WebSocket routes remain unavailable until their authority checks exist.
The relay independently verifies the first manager-issued invitation against
its promoted genesis head: manager signature and role, fixed invitation
role/key, resulting public state/core hashes, and membership-object
manifest. SQLite stages that exact candidate, rejects a missing object,
then commits the membership object, receipt, head, and cursor 2 atomically.
The published invitation API bytes pass through a restart and exact retries.
The corresponding HTTP invitation routes now stage and commit this first
issue, then serve a signed manager read of the control page and committed
membership object. A router test sends the published API request bytes
through staging and fetch, checks that an uncommitted object is unreadable,
and confirms the fetched cursor 2 entry equals the commit response. Claims,
later authority transitions, general object ACLs, encrypted batches, and
WebSocket wakes remain closed.
The next relay transition now accepts the recipient's two-signature
`invite_claim` only for that unused invitation, its exact parent head,
fixed role/key transcript, and a signed commit strictly before the
seven-day relay-clock expiry. It atomically consumes the invitation and
records the keyless pending device at cursor 3. A fixed-clock HTTP test
matches the contiguous-chain bytes and exact retry; a boundary check
rejects the claim at expiry. An unused invitation key may read the public
control chain and its issue object, then loses that read access after
claim. The pending device's own signing key can read the public control
chain but cannot fetch data objects before a grant.
The relay now also validates, against contiguous signed vectors, the next
public holder challenge and pending key proof. It checks the target and
challenge context, two ordered challenge-object manifests, resulting
pending state hashes, manager or pending signature, and proof hash. SQLite
stages both challenge objects under one exact candidate and commits their
bytes, cursor, head, and receipt atomically; the proof consumes the next
cursor without objects. The HTTP test commits both transitions with an
injected clock and exact fixture bytes. A pending device can fetch only
its addressed HPKE challenge object, never the holder's verifier object.
Admission, grant delivery, and batch data remain closed.
The relay's public admission validator now matches the next signed
contiguous vector: proved pending identity, fixed invited role, unchanged
epoch commitment, manager signature, resulting active membership state,
and distinct membership/grant object manifests. SQLite stages both
ciphertexts and commits them with cursor 6 in one transaction; the HTTP
test verifies exact fixture bytes. After admission, the recipient credential
can fetch the grant and other committed objects. The recipient client still
needs to pull this real relay history into an independent store, open the
grant, and verify data readiness. Repair, rotation, and batch writes remain
closed at the relay.

The client core now signs authenticated GET requests and strictly decodes
bounded control pages and opaque object responses against the published API
bytes. These response envelopes remain untrusted until each contained
control and object is checked against the pinned Family history. An
independent recipient-store fetch through the relay is the next proof.
An integration test now commits the first join through the relay's durable
SQLite authority, restarts it, signs the recipient's reads, downloads its
control page and manifest objects into a separate SQLite store, and opens
the admission grant only after the full verified history is present. A wrong
agreement key remains unable to open the grant, and readiness survives a
recipient restart. This uses the fixed fixture identities and calls the
relay's authenticated store boundary; dynamic claim generation, HTTP
delivery, data batches, polling, and later joins still need end-to-end tests.
The initial manager and first admitted recipient can now commit signed epoch-one
batches through the relay's HTTP route. The relay checks public signature,
Family, relay, head, epoch, device sequence, and exact batch ID reuse without
opening ciphertext, then atomically appends the envelope and signed receipt.
Authenticated result and full-log reads expose those bytes. A mixed-client
test stages a real recipient local outbox, confirms its exact retry through
HTTP, rejects a tampered POST, and fetches the batch into a manager store;
both client projections show the same record after independent verification.
This is still the initial two-device cohort: general membership, rotation,
stale-epoch rejection, delivery faults, and background polling remain open.
Fresh empty local Families can now prepare sharing with random manager
signing/agreement keys and an epoch key, encrypted under an app-supplied
local wrapping key in SQLite. The signed zero-watermark genesis candidate,
promotion manifest object, and secrets persist before POST; restart retries
the exact bytes. A dynamic manager/relay test stages and commits those bytes,
uses the manager's authenticated promotion-result read, verifies the exact
candidate and signed genesis, and reaches data readiness after another
restart. That test exposed and fixed a builder mismatch: the promotion ID
names the manifest object and result path, while the signed genesis has a
distinct transition ID. Nonempty local history
still requires encrypted promotion chunks before this path can activate.
The same fresh manager can now prepare a first invitation with a random
bearer seed and encrypted membership object, durably saving exact signed
bytes and the wrapped seed before upload. A dynamic end-to-end test restarts
the manager, commits that issue through the relay, verifies its membership
copy, creates a link only after commit, and uses the link credential to fetch
genesis and issue. A fresh recipient persists its own two keys and exact
claim before POST; the relay accepts that dynamic two-signature claim and
the recipient verifies the pending state. Challenge, proof, grant, further
invites, and background delivery remain open for dynamic identities.
The dynamic first join now continues through a persisted holder challenge,
recipient proof, and atomic admission. The manager checks the verifier
ciphertext and exact pending signature before building the grant. The
recipient stays keyless until it downloads and opens the committed grant.
In the integration test, a fresh recipient then appends a child record and
uploads an encrypted signed batch; a separate manager store fetches the
log entry and signed result and projects the same child. Candidate bytes,
object bytes, and local keys survive restarts before the relevant POSTs.
The current relay methods and builders cover only one manager and one
first recipient at epoch one. The dynamic test now sends every staged
control object, signed transition, and batch through the Axum HTTP routes,
then verifies signed reads through the relay store boundary. Exact HTTP
genesis and batch retries return the first result. Full HTTP read delivery,
lost-response and disconnected-client transport campaigns, background
polling, later invitations, removal, and recovery remain open.
Existing local records now promote as encrypted, manifest-bound chunks at a
fixed watermark. The manager stores exact chunk and genesis bytes before
upload, and a recipient verifies every chunk under its granted epoch key
before the Family becomes data-ready. The dynamic two-store test begins with
an offline child, waits for the final promotion chunk before showing that
child on the recipient, then uploads one manager edit made after the watermark
and one recipient edit. The manager marks only the promoted prefix accepted;
post-watermark local edits remain in its outbox. Multi-chunk limits and
transport-failure campaigns remain to be exercised.

The Rust core now durably prepares a first active-removal proposal from a
verified epoch-one chain. It removes exactly the target credential, preserves
the last-manager invariant, encrypts matching membership and a history
keyring under a fresh epoch-two key, and HPKE-grants that key only to the
remaining manager. The relay stages all objects and commits the transition
atomically; a missing keyring leaves authority unchanged. Both real-relay
CLI variants restart the manager before sending, verify the removed device
can read its signed control proof but not later data, retain its rejected
pending edit, rebase a manager's old-epoch batch, and accept an epoch-two
upload. This is still a first-cohort path. Android UI, automatic private
copy on verified removal, later-device enrollment, and broader rotation
remain open.

Android now exposes the first recipient's device credential in a manager
removal confirmation. The Kotlin coordinator stages the Rust proposal,
commits through the relay, verifies the new key, and resumes epoch-two
uploads. The disposable-relay emulator suite passes six tests including
this path. The runner clears the test app's prior databases before launch,
so stale Family rows from older relay instances cannot affect scheduled
job assertions. The removed Android device still lacks a sparse signed
proof/notice and automatic private-copy path.
The core now verifies a removed device's signed control page from its
contiguous pin without advancing that pin across unseen ciphertext. The
real-relay CLI test covers both a contiguous removal and one with an
old-epoch batch hidden behind the removal proof. Proof persistence, Android
notice, and automatic copy remain open.

The Android recipient now checks the authenticated public control route
before any data read. Rust saves the signed removal proof without advancing
over missing data, blocks further source edits/uploads, and transactionally
restores pending work into a distinct private Family keyed by the removal
transition. A restart reuses that copy. The real-relay CLI covers a prior
explicit copy and a cursor gap; the disposable-relay emulator verifies the
removed notice and preserved offline child across coordinator restarts.
The source Family remains locally readable and exportable.
The removed recipient can now query only its own accepted or rejected
batch result after losing a POST response. The core verifies that receipt
against the exact saved envelope without clearing the archived source
outbox. Android distinguishes never-uploaded, accepted, rejected, and
unknown delivery when showing the new private Family.
The [public-authority factoring](../topics/sync-and-encryption.md#proposed-implementation-seam-for-general-relay-authority)
now supports ledger-backed general relay routes. Its initial
[Daybreak advisory](#advisory-general-authority-seam-review-at-bb68156)
failed; the corrected implementation passed the focused
[recheck](#general-authority-advisory-pass-at-98fa0ea). This does not
substitute for the end-of-M0 gate.

The local SQLite schema now migrates the first invitation's exact prepared
bytes into a per-invitation table on version-one upgrade. The first-invite
path uses that table. A manager can prepare a second invitation against the
verified shared head, reopen its exact candidate and membership object,
commit both through the real relay, and obtain a link only after verifying
the signed issue and encrypted membership. The CLI also commits a new
invitation from the remaining manager after epoch-two removal and rotation.
The recipient core now verifies every signed control before the later issue,
checks each signed global cursor, and saves the complete sparse prefix with
its encrypted credentials and exact claim in one transaction. Android fetches
bounded control pages before calling that verifier. The existing two-emulator
first-cohort flow passes with the paged join path. A real multi-page relay
scenario and later key handoff were still open at that stage. The CLI
commits and retries the later recipient's exact claim through HTTP after a
simulated page boundary; it
remains keyless pending admission.

The next general-device pass follows these ordered implementation proofs:

1. Replace the Family-wide first-invite slot with durable per-invitation
   preparation. Build against the current verified control head and epoch,
   retain exact staged bytes through restart, and reveal a link only after
   its signed issue commits. A stale or removed manager cannot issue.
2. Let a joining device page the entire filtered public control ancestry
   through the issue named by its link. Verify every signed transition and
   cursor across intervening batches and page boundaries, then atomically
   save that prefix with its new credentials and claim before the first POST.
   A pending device remains keyless until grant and full-log hydration.
3. Extend holder challenge, proof, admission, repair, and epoch-key recovery
   to that later device, including a grant after rotation. Reuse the shared
   public verifier for both relay acceptance and client replay.
4. Run a three-device real-relay scenario with delayed wakes, restart,
   multi-page controls, cancellation/expiry, role changes, and removal.
   Review its implemented authority and recovery boundary at the M0 exit
   gate before using real Family data.

- [ ] Share deterministic public authority replay between client and relay
  without giving the relay epoch keys or event semantics. Preserve reviewed
  first-cohort bytes and GET/write ACLs; add the public history ledger,
  two-phase verification, candidate-scoped staging, exact retry first, and
  global cursor CAS before third-device and general role/removal routes.
- [ ] Implement one ordered Family log, signed control receipts, atomic
  compare-and-swap, object manifests, accepted device sequences, signed
  batch receipts, and authenticated reads. Relay stores opaque bytes and
  cannot decrypt marker strings in events or logs.
- [ ] Implement invitation bootstrap, seven-day honest-relay expiry,
  authenticated claim retry, holder challenge/proof, atomic admission and
  grant, grant repair, and background sync state machine. Use injected wakes
  and delayed delivery, without a simultaneous-online or second-approval
  dependency (FS19-FS21, FS35-FS39, FS47, FS51, FS55, FS59).
- [ ] Implement rotating active removal, keyring verification, stale-writer
  handling, signed removal proof, and one private-copy transaction. Exercise
  manager-removal orders, third caregiver, pending work, forged denial,
  cloned credential, and malicious fork (FS04-FS12, FS34, FS52-FS58).
- [x] Run the early M0 implemented-protocol review from the
  [MVP plan](../mvp-plan.md#security-review-gates) using the
  [review runbook](../security-review-runbook.md). Review authorization,
  invitation, encryption, and rotation at a fixed revision. Fix access and
  retention blockers before expanding mixed-client sync.

The v1 `remove_pending` control now has a durable manager-side builder,
native binding, and Android access action. It removes exactly the selected
invitation/device row without rotating the epoch. A claimed recipient that
was removed before receiving a key may read signed public controls using
its historical device key, verify the removal, and see that its join ended;
object and batch reads remain denied. A real-relay emulator case drops the
accepted manager response, reopens the coordinator, retries the exact
candidate, and verifies the stopped recipient after restart. A second
emulator case taps the manager access button. The no-object FS71 vector
already fixes kind 9 bytes; the broader mixed-client and independent M0
exit gates remain open.

Gate: the relay and clients enforce the reviewed authority transitions;
deterministic authority and key-handoff tests pass; the independent review
has no unresolved access or retention blocker.

### 4. Mixed-client sync and recovery adversarial pass

The real-relay CLI now recreates the server router after an accepted POST
whose response the recipient loses, and again halfway through staging a
multi-object removal. Exact result lookup and idempotent stage/commit
continue across both restarts in all three first-cohort flow variants.
This exercises durable relay state, not an OS power cut or all transaction
boundaries. Browser recipient credentials, more clients, and the bounded
fault matrix remain open.
The browser IndexedDB harness now replays a signed same-epoch invitation
through the Rust authority and data view after reload. A rotation still
requires verified new epoch keys and is outside this initial-manager path.
Its real-relay queue case uses two distinct dependent edits, loses the
response after the first queued upload is accepted, and checks that the
saved envelope is retried exactly after reload before the final edit drains.
This covers an uncertain queued upload for the initial manager. A separate
browser recipient case replays fixture-fed admission controls, fetches its
committed HPKE grant from a disposable real relay with its own signed GET,
stores its signing/agreement/epoch keys in IndexedDB, then uploads an
encrypted child entry after browser reload and verifies signed acceptance.
An independent browser manager store at the same verified authority head
pulls that recipient batch from the relay and reads the child name. This
proves a recipient-authored encrypted event reaches another device without
sharing the recipient's IndexedDB state. The admission controls and keys
are still fixture-provisioned. A separate dynamic first-cohort browser/native
flow now covers claim through grant and bidirectional encrypted edits over
relay HTTP. Later epochs and the mixed-client fault matrix remain
open.
The core now runs eight deterministic seeded encrypted logs with 64 child
edits each, including deletes, restores, and skewed HLC values. At every
eighth cursor, a fresh replay matches incremental projection byte-for-byte;
duplicate application fails without changing state. This is a bounded
rebuild/convergence property check, not a randomized multi-client relay
campaign.
The Android growth form writes grams and millimetres through the Rust
local/shared operation path. A local restart and file restore preserve the
measurements; the two-emulator relay check confirms the manager reads a
recipient-created growth entry with both values.
The Celsius path now uses Rust decimal conversion to published hundredths
with half ties away from zero, preserving entered text in the timeline.
Local restart/file restore and the two-emulator recipient-to-manager check
cover the entry.
An injected late SQLite failure during removal-triggered private-copy
creation now proves the copy Family, source mapping, origin, and operations
roll back together across reopen. Retrying the same copy after removing the
fault succeeds and preserves the child. This covers one transactional fault
point; OS power loss and the broader cut-point matrix remain open.

- [ ] Promote all local history atomically. Connect two CLI clients and the
  real-browser harness through the relay. Inject offline writes, duplicate
  requests, lost WebSocket notices or responses, polling fallback, client
  and relay restart, old-epoch rejection, malformed signed payloads, and a
  withheld latest batch (FS03, FS17-FS18, FS31-FS32,
  FS46, FS49-FS50, FS60).
- [ ] Run the end-of-M0 independent review using the
  [review runbook](../security-review-runbook.md). Review recovery,
  pending writes, crash safety, isolation, and scenario coverage at a
  fixed revision. Map each finding to a scenario/vector and fix access or
  retention blockers before M1 uses real Family data.

Gate: the bounded real-relay scenario suite passes with crash/restart and
multiple Families; mixed clients converge on verified history; an unknown
batch outcome retains its outbox until accepted evidence is retrieved; the
end-of-M0 review has no unresolved violation of an agreed product promise.

## Test placement and CI

The Rust core implements canonical CBOR and structural version-1 operation
decoding, with Family/author binding and unknown-field byte retention.
`cargo test -p babytrack-core` runs CB01-CB06 from
`tests/vectors/records-v1.json`, plus fixed operation and minor-field cases
from `full-wire-v1.json` and `negative-batch-v1.json`. Domain hashes,
Ed25519, and XChaCha20-Poly1305 pass their known answers in
`crypto-v1.json`; supplied-nonce encryption still needs safe nonce ownership
in the batch/outbox layer. HPKE opens the fixed `HPKE01` ciphertext and
round-trips a seeded sender, but the crate's public sender API cannot inject
the fixture's raw ephemeral private key, so exact fixed seal bytes are not
yet reproduced. `BATCHBYTE01` matches header, plaintext, AAD, ciphertext,
signature, envelope, and object hash exactly, and verifies/decrypts back to
the original operation. The in-memory Rust projection runs CROSSMINORBYTE01
and INERTBYTE01/PRECREATEBYTE01/WRONGSCOPEBYTE01/PREFSBYTE01: unknown field
500 retains its canonical bytes, valid replay rebuilds identically, and a
signed malformed batch consumes its cursor without applying operations.
Replay also advances across separately verified control cursors in the same
ordered stream.
Published v1 field checks include UNIT01-UNIT03 and exact rational
conversions. The core also generates a per-Family HLC with the published
tie, future-stamp, counter-rollover, and maximum-wall behavior. Storage must
persist its state with each local operation. The caller still owes
receipt/authorization checks and an atomic
log/projection transaction. `bash scripts/check_wasm_smoke.sh` builds the
wasm-bindgen Node binding and runs the fixed encrypted Family and child batch,
unknown minor field, four inert cases, and wrong key/signature checks through
JavaScript. `bash scripts/check_native_smoke.sh` generates UniFFI bindings
and runs the same selected encrypted cases through Kotlin/JNA and Swift on
macOS. All four runtimes seal the fixed child and Family operations to the
exact expected encrypted envelope bytes, then verify, decrypt, and project
them. Wrong key, signer, Family, relay, and tampered envelope signature
are rejected without advancing the projection cursor in each runtime.
The caller supplies the fixture's nonce, batch ID, and sequence;
production allocation and retry ownership belong to the durable outbox.
Primitive batch replay and fixed-header sealing in native/wasm bindings now
require the explicit `fixture-api` build feature. Default generated bindings
exclude them; `bash scripts/check_fixture_api_boundary.sh` checks this.
A production core-owned authorization session and durable retry API remain
required before sharing UI work.
The core operation encoder also drives a signed encrypted multi-batch replay:
cursor order beats an extreme HLC, tombstoned fields stay retained, restore
is explicit, identical operation bytes dedupe, conflicting IDs and second
creates become inert, and replay from the beginning matches incremental state.
`LocalProjection` now applies validated local-only operations by contiguous
append index, rejects invalid or cross-Family edits before mutation, and
rebuilds to the same state in `cargo test -p babytrack-core`. Durable storage
has begun: native `SqliteStore` atomically appends local-only operation bytes,
their index, and HLC state; reopen rebuilds the projection from the journal.
`core/tests/sqlite_store.rs` covers failed append, duplicate ID, restart,
and two-Family isolation. A reusable browser IndexedDB local-only journal
now stores validated operation bytes and append indexes in one transaction;
`WasmLocalFamily` replays through the shared Rust projection. The real
Chromium smoke covers reload, duplicate rejection, and wrong-Family append
rollback. Materialized projection, browser HLC, general shared replay,
general browser outbox, and private copy remain open. Native SQLite now also reserves a random
batch ID and nonce and persists exact signed envelope bytes in one transaction.
The browser also has a separate production public-authority journal. Its wasm
verifier uses the Rust control chain for signed genesis, controls, and batch
acceptance; IndexedDB stores exact bytes with cursor/head atomically and
rebuilds the verifier after reload. The Chromium smoke rejects a forged
receipt and duplicate batch without advancing the durable pin. A bounded
browser pull now signs exact GET paths with caller-held manager credentials,
decodes log pages and batch results in Rust, and commits each verified entry.
The Chromium fixture transport exercises a control and an accepted batch;
the browser smoke also starts a disposable relay, commits a fixture genesis,
and fetches its signed object. A storage-independent Rust initial-epoch replay
checks the genesis manifest and epoch-key commitment. Production wasm then
seals a manager operation with a fresh random batch ID and nonce. Chromium
stores exact bytes in IndexedDB before upload, retains them after a simulated
lost response, retries after reload, and atomically clears the outbox only
when a signed acceptance enters public history. Before upload, Rust overlays
the saved operation on the verified cursor-one view, including after reload.
It projects the accepted Family record and rebuilds it after reload.
The low-level staging call rejects a different edit while exact upload bytes
are pending; a separate browser queue now saves later validated operations
behind that batch. Rust previews their ordered overlay through reload.
After each signed acceptance, Rust seals the next draft against the new
verified head, then one transaction saves that envelope and removes the
draft. A Chromium real-relay case accepts a child creation, queues two
later changes, and drains them in order. A structurally valid edit of an
absent record is rejected before sealing and leaves the outbox empty.
The test-only native CLI example verifies that accepted browser batch from
signed genesis and its object, authors a second encrypted child batch, and
Chromium fetches and projects the child after reload. Both use the same
fixture manager credential. A second real-relay case uses an independently
admitted browser recipient credential for a signed grant fetch and encrypted
upload after reload. Its admission controls are fixture-fed. The dynamic
first-cohort flow below covers live claim through grant. Rotations, queued-edit
repair after rejection, and later-epoch mixed-client transport remain open.
The browser now also opens a real invitation fragment, signs a bounded
public-control GET with its invitation credential, and verifies the linked
genesis and issue through the Rust sparse authority chain before saving
the exact page and head pin in IndexedDB. A disposable relay stopped after
issue supplies those bytes; Chromium replays them after reload and rejects
a changed relay origin or tampered saved page. The same browser case now
generates its device keys and exact claim through a wasm-enabled Rust builder,
saves them before POST, and verifies the relay-signed committed claim after
reload. A simulated lost commit response retries the saved bytes and reaches
the same committed cursor. That case ends at the claim; the separate
dynamic holder flow below continues through admission and data readiness.
After claim commit, the browser switches from the consumed invitation
credential to its saved pending-device signing key for exact control reads.
The real relay accepts repeated empty pending polls after reload without
duplicating a page. The dynamic flow below uses this polling for challenge
and grant delivery.
The recipient proof builder is now a pure Rust core path shared by native
and wasm. A browser case replays the published signed challenge prefix,
fetches the committed HPKE object from a real relay with its device key,
checks the object against the signed manifest, and produces the published
proof candidate bytes. A wrong agreement key fails before any candidate is
made. This byte case uses fixture-fed preceding controls. A second disposable relay stops
after a fixture claim and holder challenge. Chromium saves that fixture claim,
pulls the challenge with its pending-device credential, saves a newly signed
proof candidate before POST, and retries identical bytes after a lost result
and reload. The verified proof reaches cursor five. The dynamic flow below
continues with a native holder grant.
For an already committed fixture grant, the browser now carries its verified
sparse admission into a separate contiguous public-history store. It uses
the admitted device credential to fetch the full log from genesis, then
hydrates the signed genesis and addressed grant objects, verifies the epoch
key in Rust, and reopens a ready data view after reload. A proof without a
grant cannot activate. The dynamic flow below exercises the same hydration
with newly generated claim, proof, and grant bytes.
The browser harness now also runs one fully dynamic first-cohort join. A
separately persisted native holder creates the Family and invitation, reads
the browser-generated claim, commits a challenge, verifies the browser's
new proof, and commits the grant. Chromium pulls each step through relay
HTTP, hydrates full history after admission, and reopens its ready view.
The holder then writes an encrypted child batch that Chromium decrypts; the
browser writes a different encrypted child batch that the holder verifies
and decrypts. After initial disposable relay setup, both clients stage,
commit, and read through HTTP. Later epochs and the broader offline fault
matrix remain for the M0 mixed-client exit gate.
The first manager's signing and epoch keys persist in IndexedDB and are
reloaded for this narrow proof.
An uncertain retry returns those bytes after reopen even if the caller's
current head/key has changed. The initial `FamilySession` stages through
that path only with its verified manager identity, head, epoch key, and
signing key. A matching relay-signed acceptance moves the exact envelope and
receipt to replayable SQLite history while clearing the outbox in one
transaction; a bad receipt leaves it pending through restart. The newer
manager-ready session stages against its fully checked current head, key,
and signing identity. It reports an exact retry while a prior batch is
uncertain, even after rotation. A signed stale-epoch rejection matching the
newly pinned head archives the old bytes and receipt without removing the
local operation; the next stage reserves a new nonce and batch ID at the
same sequence. Restart retries the replacement bytes exactly. General
general browser outbox and removed-device private copy remain open. In the public
shared journal, an own accepted batch now advances the global cursor,
stores the signed envelope and receipt, clears its exact matching outbox,
and advances the local accepted index/sequence in one SQLite transaction.
A forced failure during outbox deletion rolls back both the cursor and
local progress. A new local operation then stages at the next sequence,
even after other devices' control entries interleave. The legacy initial
`FamilySession` remains a narrow epoch-one path and must not be used for
later control history.
The decoder now enforces UUIDv4 Family/device/batch identities, UUIDv7
operation/child/activity identities, and a positive batch epoch.
The Rust core now verifies the fixed `GENESIS01` committed control object:
relay pin and receipt signature, manager signature, transition core and
state hashes, promotion manifest binding, receipt context, and head hash.
The initial manager's key is checked against its committed genesis
commitment. The separate shared journal now persists later control heads;
rotated keys require complete grant, keyring, and membership verification.
The public `ControlChain` now verifies the first `invite_issue` after
genesis against the contiguous-chain vector: manager role/signature,
relay-signed receipt, parent head, global cursor, state/core hashes, and
canonical invitation row. Replaying the same transition or altering its
declared state cannot advance. The recipient claim now requires both the
invitation and new device signatures, an unchanged issuer manager, matching
claim transcript and pending state, and a relay-signed commit strictly before
the seven-day expiry; a boundary test exercises the final millisecond and
the exact expiry. Public chain replay now also checks a holder's challenge
and the pending device's proof against the latest pending row, committed
hashes, signatures, and cursor. Public admission
now atomically replaces a
proved pending device with its active row at the invitation's fixed role
and commitment; repair names that admission and leaves authority unchanged.
The control dispatcher now covers all v1 kinds except genesis (the explicit
starting root). Fresh signed tests cover canceling an unused invitation,
removing a keyless pending enrollment without rotation, promoting a member,
demoting a manager while canceling that issuer's unused invitations, and
rejecting demotion of the final manager.
The core now creates the exact v1 invitation fragment only from a verified
committed issue, and a recipient parses its strict canonical CBOR/base64url
descriptor without contacting the relay. It pins the exact origin, relay
key, Family/genesis, role, invitation seed, and signed issue hash, then
verifies the fetched genesis and issue before a claim. The contiguous
fragment round-trips exactly; altered link fields, padding, extra fragment
keys, and mismatched signed bytes fail. A recipient now prepares one
Family-scoped signing key, agreement key, enrollment nonce, and exact
two-signature claim candidate, and atomically saves them before a POST.
The secret bundle is encrypted under an app-supplied local wrapping key;
reopen reproduces the same signed bytes and public keys, while a wrong
wrapping key fails. A synthetic relay-signed commit of the prepared claim
passes public authority replay. Android still needs to supply the wrapping
key through its platform key interface. Resume also rejects a committed
claim for the same invitation whose exact signed candidate differs from the
saved attempt. Candidate retry/rebase after a definite stale-head response
remains open.
The contiguous byte vector passes through both transitions. Producing
transitions remains open. A joining device can now use its saved agreement
private key to open only its committed admission grant, then rebuild the
same manifest-bound Family view as the manager. It stays keyless before the
grant, projects the recipient batch after admission, and becomes unready
after verified removal. Its saved signing key can stage local work only for
the matching Family/device. This initial recipient path covers admission
into epoch one; admission after a rotation still needs historical keyring
delivery and validation.
Public replay now also
verifies the recipient's accepted signed batch at cursor 8, then the
manager's active-removal transition at cursor 9. Removal increments the
epoch, keeps a manager, cancels affected unused invites, clears pending
challenge/proof state, and rejects a fresh old-key write from the removed
device. The rotation keyring and recipient grants still need cryptographic
checks before epoch-two data can be used.
The committed holder-challenge manifest now yields an opaque challenge
context in the core. The pending device checks the addressed HPKE object's
hash, claimed agreement/signing keys, context, and committed secret hash
before signing the proof. A holder checks the verifier object's hash,
committed epoch key, challenge hash, pending signature, and committed proof
hash. The contiguous chain's actual object bytes pass; wrong keys and
tampered bytes fail.
The admission manifest now yields an opaque grant context after the signed
transition. The recipient opens only the addressed HPKE object using its
committed agreement key, checks purpose/suite/context and full object hash,
and accepts an epoch key only when it matches the signed commitment. The
contiguous join object's actual bytes pass; wrong keys and tampering fail.
Repair and rotation grant opening remain open.
For issue, admission, repair, and removal, the core now also binds each
encrypted membership object to its signed manifest and decrypts it only
under the committed resulting epoch key. Its transition ID, prior head,
resulting state hash, epoch, and delta must exactly repeat the public
transition; the contiguous chain's objects pass, while tampering and an
old epoch key fail. The addressed repair HPKE grant now binds its purpose,
original admission, current recipient key version, transition context,
object hash, and unchanged epoch commitment; the contiguous repair object
opens to the same epoch key and a wrong private key fails. Core session
enforcement of readiness remains open.
The removal manifest now yields an opaque rotation check. Before activating
epoch two, it requires exactly one hashed HPKE grant for every remaining
active device, rejects a grant to the removed device, opens the addressed
new key against its signed commitment, and opens the hashed keyring under
that key. Every earlier numbered key must be present once and match its
pinned commitment. The contiguous removal objects pass; a removed device
or tampered keyring fails. Durable rotated-key storage and full session
readiness remain open.
The public rotated-key path now also verifies the encrypted membership
object and cannot use the initial-epoch commitment shortcut. Opening a
keyring rejects a raw key repeated from an earlier epoch. Incremental
authority replay records historical transition, manifest object, and device
IDs and rejects reuse, including object reuse across kinds. These registries
must still be rebuilt from a durable verified log on restart.
Native SQLite now pins the signed genesis and every subsequently verified
public control or accepted batch entry in one global cursor stream. The
`PublicHistorySession` replays every byte on reopen, rebuilding historical
IDs and comparing the derived head/cursor with the durable high-water pin.
The contiguous nine-cursor vector survives a restart after each entry,
including the interleaved recipient batch; a missing stored suffix and a
replayed older prefix fail without lowering the pin. This is an observed
public-authority journal only. It does not yet make a device data-ready,
persist enrollment credentials or keyring, project remote data, or clear a
local outbox atomically with its accepted shared entry.
Committed manifest objects now persist only after their hash, length, ID,
and transition match signed history. A manager-only ready replay requires
every declared object, decrypts membership, opens the complete rotation
grant set and keyring, and then projects each accepted batch in the same
cursor order. Withholding one grant keeps it unready without lowering the
public pin; the completed projection survives restart by rebuilding from
the verified log and objects. This is not yet a joining-device session,
secure durable credential/key storage, or local outbox confirmation.
`FamilySession` now consumes the fixed genesis and `BATCHBYTE01` acceptance
through a core-owned path: it verifies the manager, head, epoch, sequence,
object hash, cursor, and relay-signed receipt before projecting the first
shared event. Tampered receipts/envelopes and duplicate replay leave the
cursor unchanged. A separately signed but undecryptable manager batch is
recorded inert under the verified epoch key, then a later valid batch
applies at the next cursor. This currently covers only epoch-one manager data after
genesis; later members, rotation, durable shared replay, and malicious relay
fork checks remain open.
`bash scripts/check_browser_smoke.sh` launches Playwright's isolated Chromium
shell, loads the wasm binding, and checks IndexedDB reload, an aborted
multi-store transaction, and Family-scoped keys and rows. It uses fixture
keys for encrypted batches and the reusable local-only journal adapter. A
production public-authority adapter also verifies a signed acceptance through
Rust and replays it after browser reload. The same smoke starts a disposable
relay and fetches an accepted batch through signed browser GETs; complete
browser sync storage and cross-language vector coverage remain open. It also
stages a production encrypted write, loses the POST response, retries exact
bytes after reload, and clears its outbox on signed acceptance. The native
CLI example opens those browser bytes and contributes an encrypted child
that Chromium then reads through the same relay.

Every agreed FS case gets a deterministic core/relay action binding with
assertions for local state, shared state, pending outbox, visible status, and
available next action. The relevant slices above assign the first binding
owner. FS31-FS32, FS46, and FS56 use a malicious relay; honest-relay cases
do not inherit those stronger assumptions. Selected UI observations are
reused later: M1 Android local/sharing/recovery cases, M2 web Family and
browser-key cases, M3 iOS parity, and M4 watch targeting. M0 records actual
runner commands next to its completed gate; the passing byte subset does not
establish any complete scenario or slice gate today.

The [MVP CI plan](../mvp-plan.md#ci) requires core, bindings, protocol,
scenarios, vectors, dependency, and CI edits to run the bounded relevant
checks on PRs. The current foundation workflow runs all jobs on every push
and PR; introduce path selection as component suites grow. Randomized fault
sequences and fuzzing run nightly with saved seeds. Physical device startup
and size baselines begin in M1; simulator timing is informational.

## Advisory first-cohort sync review

Daybreak Blue at high thinking reviewed fixed commit
`5ada80cfbf76d7a5c18d23e9c6e6a804a392c67c` in read-only plan mode
through Yep Anywhere session `01a0dffd-fe08-7d92-a22d-ca06d89dc3db`
(process `940df9e3-caca-48a3-9772-cb296aeaf0ca`). The separate checkout
remained clean. The reviewer ran `cargo test -p babytrack-cli --test
dynamic_creation --locked`, `cargo test -p babytrack-core --locked`, and
`cargo test -p babytrack-server --locked`; all passed. Result: **advisory
FAIL** for first-cohort eventual sync, not the formal early M0 gate.

| Finding | Disposition |
|---|---|
| High: a manager batch durably staged against genesis can be rejected before admission and then forever rejected for a stale exact head, despite the same epoch and continued authority. | Blocker. Let manager batches commit from genesis, verify a known same-epoch ancestor and continuous authorization, and remove fixed join cursor positions. Add FS49/BATCH06 with a batch staged before joining and controls interleaved with batches. |
| Medium: identifiable definite batch rejections return unsigned HTTP 409 and have no durable result. A cloned credential's conflicting sequence can leave an exact outbox item stuck. | Blocker. Persist signed rejection receipts and expose exact retries/results; after verified competing acceptance, retain the operation but archive/resequence its old envelope. Bind BATCH02/BATCH04 and lost-response variants. |

The high finding is now covered by two real-relay HTTP paths in
`cli/tests/dynamic_creation.rs`: a genesis-sealed manager outbox survives the
whole join and uploads exact bytes afterward, and a manager batch commits at
cursor 2 before the invitation, shifts every later control cursor, and is
verified by the recipient before its claim. Relay authority indexes controls
by order, accepts a continuously authorized manager's same-epoch ancestor,
and the core replays interleaved entries before preparing later controls.
The follow-up below found that this test supplied the pre-invitation batch
directly from the manager test process; the recipient could not fetch it
under the actual pending-reader ACL. The pre-invitation transport path is
therefore still open.
The medium finding now has a real-relay credential-clone regression: one
accepted batch and a competing exact batch receive distinct durable signed
results, a lost response resolves by batch-ID query, and the rejected copy
retains its operation until a verified competing prefix permits resealing at
the next sequence. FS62/BATCH07 records the case. This closes the
sequence-conflict finding for the tested initial cohort; later epoch and
removal variants remain in the M0 gates.

The reviewer found no key/plaintext disclosure to relay storage, keyless
admission, forged device authorship, or deletion of committed local
operations in this subset. It called out unexecuted multi-chunk boundaries,
mixed-version exchange, removal, repair, polling, private copy, and browser
outbox as remaining work. Its threat assumptions match the
[Family trust limits](../topics/family-sharing-and-trust.md#accepted-trust-limits):
local keys stay protected; a malicious relay can withhold or fork history
and lie about its clock. The newer HTTP-write integration test at `ba840d8`
addresses one transport coverage gap, but neither blocker above.

Readable and password-protected portable snapshots now export and restore
local-only Families through the core. The FILEBYTE08 fixture validates the
readable format. The protected form uses the fixed Argon2id/XChaCha profile,
NFC passwords, an app-supplied memory budget check, and one failure category
for a wrong password or corrupt ciphertext. A restore publishes a new
Family and its saved-point/gap metadata in one SQLite transaction; source
authority is not copied. Shared pending work, private copy, browser parity,
and UI wiring remain open.

## Advisory first-cohort follow-up

Daybreak Blue at high thinking reviewed fixed commit
`42cd7335cf89d8fe4721737c822835c7f5032845` through Yep Anywhere
session `01a0e03e-9b0e-70b3-b16d-020cf8f6de0a` (process
`9936c5ef-00c3-4c1a-a7f7-2e0c14a56b6f`) in read-only plan mode.
The checkout remained clean. The reviewer ran the dynamic CLI, local API,
server, shared-history, and portable-file Rust suites; all passed. Result:
**advisory FAIL**, not the formal early M0 gate. Assumptions were an honest
ordered relay, hostile enrolled device, compromised relay storage, and a
malicious relay able to fork, withhold, or lie about time under the agreed
limits.

| Finding | Disposition |
|---|---|
| High: when a manager batch commits before invitation, the pending reader sees sparse controls but cannot fetch the intervening data. Contiguous `PublicHistorySession` refuses the issue, so the recipient cannot claim. The original test injected the batch from the manager store. | The recipient now persists a signed sparse control ancestry across restarts, can claim and prove without reading the intervening batch, and cannot become data-ready until it fetches and verifies the contiguous full log and batch receipt after admission. The separate-store FS49 regression uses actual relay HTTP GET routes and checks the pending `/log` denial. |
| Medium: Android reads the entire selected file before validation; Rust collects every line pointer before row-limit enforcement. | The parser now bounds its line index before collection. Android reads streams to a cap based on current available memory, at most 128 MiB, before UniFFI; an unknown-length provider test verifies early failure. Larger valid files need a later streaming native API. |
| Medium: Android reports only transient save/restore success and does not show the file's snapshot point or later unsaved changes. | The app now persists a completed-save timestamp and local revision only after the output stream closes, compares that revision on restart, previews snapshot time, record count, and known gap before restore, and shows restored origin afterward. An emulator save, later edit, restart, preview, and new-Family restore passed. |

The reviewer found the signed sequence-conflict rejection and verified
resealing coherent for the implemented first-cohort trace. A relay/client
restart after rejected-result loss remains a useful additional regression.
General membership, rotation, stale-epoch recovery, Keystore wrapping, and
background sync were outside this advisory; it did not pass those gates.
The bounded-read implementation passes the portable-file Rust test, Android
JVM unit tests, and an Android debug APK build. The Android UI's temporary
memory cap can be below the 2 GiB v1 file maximum; it reports that device
limit rather than risking process termination. The full 2 GiB contract
requires a streaming import path before it is claimed on Android.

The shared Rust core now has a bounded active-device full-log pull that takes
an app-owned authenticated GET transport. It verifies each contiguous control
or batch against the signed relay result before advancing the durable cursor.
An unknown batch-result fetch leaves that cursor at the last verified entry;
the FS49 HTTP regression restarts the recipient and retries from there.
The core now separately hydrates missing objects from the verified control
manifests with a per-pass budget. A denied promotion chunk leaves readiness
pending, a substituted object kind is rejected, and the FS49 HTTP regression
reopens the store and resumes object delivery. The pull and hydration steps
alone never establish data readiness;
the ready session replays and opens every required byte. A
`no_more_visible` result reports the relay's page claim, not proof that a
withholding relay has no hidden suffix. Network scheduling and background
polling remain open.

The relay now also accepts the existing signed read CBOR in an empty-body
Authorization header, with exact lowercase-hex syntax. Android's standard
HTTP transport otherwise rewrites a GET with an output body to POST. The
server's fixed API vector and HTTP test cover the header, the legacy body,
and rejection of a request carrying both. This changes only transport
placement; Rust still verifies the same signed method, path, and authority.

## Advisory sparse-join crash follow-up

Daybreak Blue reviewed fixed commit
`50c93ac65659d7dd32acce44ff34af0097ad5c2d` at high thinking through
Yep Anywhere session `01a0e061-71fc-7de2-abb6-a811b5509eda`
(process `15d66723-ea1e-4b33-aae7-cba76a950b83`) in a separate read-only
checkout. It ran the focused FS49, full CLI, core, and relay suites; all
passed. Result: **focused advisory FAIL**, not the formal early M0 gate.
The sparse control ancestry, pending read ACL, and post-admission data
readiness checks passed. The reviewer found one medium crash window: recipient
identity and claim were committed before the genesis shared-history root in
a second transaction. A crash between those commits left `resume` unable to
open the durable attempt. The repair writes the Family, local sync row,
shared root, encrypted attempt, and sparse issue in one SQLite transaction.
A failure injected on the final insert proves rollback of all five rows;
the FS49 separate-store flow still resumes the exact claim after restart.
FS51 now names this boundary explicitly. No wire change was needed. The
follow-up below reviewed the repair.

### Sparse-join repair follow-up at `7e4a8a6`

Daybreak Blue at high thinking reviewed fixed commit
`7e4a8a63765189d4832f791cb01c03c7dff5f475` through Yep Anywhere
session `01a0e073-1e36-7ad3-b8a2-30d4d844f59a` (process
`50a71b07-3f2d-4303-8f41-44b0c0881ef1`) in a separate read-only checkout.
Its focused atomicity, recipient restart, and real-relay CLI tests passed.
The intended FS49 sparse route and ordinary zero-gap route passed review,
including exact key and claim retry, pending ACL, post-admission readiness,
and the pinned relay identity. Result: **focused advisory FAIL**, not the
formal early M0 gate, for one low availability flaw in the unused public
`prepare_with_batches` helper: a crash after writing its attempt but before
writing its nonempty prior-batch prefix made `resume` reject the gap. That
helper is removed. A pre-invitation data gap uses the existing durable sparse
route, which the real-relay FS49 test exercises; ordinary enrollment accepts
only the genesis-to-issue zero-gap route. No wire change was needed. The
reviewer did not run a full workspace suite, power-cut campaign, or migration
from already-wedged databases; those remain outside this focused review.

## Advisory authority and key-handoff preflight

Daybreak Blue at high thinking reviewed fixed commit
`dd138166da631064064817d17ee508249da033be` in read-only mode through
Yep Anywhere session `01a0df66-09ef-7042-9e95-5ee4bea9e8a4`.
The checkout matched that commit and stayed clean until verified idle. The
reviewer inspected source, contracts, vectors, and scenarios without running
tests. Result: **FAIL for integrated client authority readiness**; the
cryptographic transcript checks themselves were found sound. This advisory
does not close the early M0 gate.

| Finding | Disposition |
|---|---|
| Durable pin, pending enrollment credentials, grant status, keyring, and removal proof are absent. A restart can forget a higher observed head. | Blocker for relay-backed client use; build transactional, core-owned authority replay and restart rollback tests (FS09, FS37, FS51, FS56). |
| Native store and initial `FamilySession` cannot persist interleaved control and other-device data at one global cursor. | Blocker; replay the full contiguous chain through a durable store with restart at each entry (FS18, FS48, FS60). |
| A signed stale-epoch rejection leaves the exact pending outbox batch stuck forever. | Blocker; preserve uncertain bytes, then atomically re-batch only after verified rejection/rotation or archive for private copy (FS48, FS54, FS57). |
| Public commitment verification could issue a rotated epoch key token before grant, keyring, and membership checks. | Blocker; restrict initial-key verification to epoch one and issue rotated capability through one complete check. This change begins that fix; durable session enforcement still follows. |
| Bootstrap parser, cancel/role/pending removal handlers, and verified private copy are absent. | Planned work required before the early gate; add negative bootstrap and lifecycle cases (FS19–FS21, FS35–FS38, FS47, FS51, FS54–FS55). |
| Historical device, transition, object, and grant IDs can be reused. | Hardening; add durable ID registry and IDREUSE01. |
| A rotation can reuse an earlier raw epoch key under a different epoch commitment. | Hardening; reject equality during full keyring opening. |

The reviewer did not recommend a v2 wire change. The agreed v1 contract
remains the target. See [the review runbook](../security-review-runbook.md)
for the launch and evidence procedure.

## Advisory Android sync follow-up at 5e45858

Daybreak Blue at high thinking reviewed fixed commit
`5e458584d8df68f14e7bf1e35611428b0f6e2e04` in read-only detached
checkout through Yep Anywhere session
`01a0e0c8-bb0a-7ce2-806e-94279fb25257`. It verified idle and a clean
checkout. The reviewer ran 55 core tests, 2 CLI tests, 10 relay tests, and
an offline Android build; it inspected but did not run emulator tests.
Result: **focused advisory FAIL**, not a formal M0 gate result.

| Finding | Disposition and regression |
|---|---|
| High: a signed stale-epoch rejection naming rotation cursor R became unresolvable after unrelated accepted history advanced to R+1. | Rust now replays the verified historical prefix through R, checks the signed head/epoch/sequence there, and archives at the current pin. The FS48/BATCH04 test adds a later accepted epoch-two batch and restart; it reseals retained work with fresh bytes and current sequence. |
| High: a saved pre-commit recipient claim could not be retried after restart without reentering the bearer fragment. | A Family-scoped binding reopens the encrypted attempt and exact candidate. Foreground and scheduled recipient passes retry it before pending-device reads. The Android real-relay test drops a claim before POST and separately drops a committed response, then resumes by Family ID with empty fragment state (FS38/FS51). |

The reviewer found the prior sequence-conflict recovery, committed-claim
restart, inert-batch warning, and relay-confirmation wording substantially
resolved. It requested a real-relay injected hostile-batch warning proof and
broader FS62 client/relay restart cases as hardening. The focused follow-up
passed a new fixed-revision check below; the early M0 gate still waits for
general authority and rotation. Accepted malicious-relay limits remain as
described in the trust topic.

### Advisory Android sync recheck at 00e827b

Daybreak Blue at high thinking reviewed fixed commit
`00e827b296059f51bea944a05569b1e092f25627` in read-only detached
checkout through Yep Anywhere session
`01a0e0de-3e81-7ba3-b459-e9d834df75c6`. The process reached verified
idle, with no tracked review edits. Result: **focused advisory PASS** for
the two prior high findings; this is not the early M0 security gate. The
reviewer ran focused Rust history/enrollment tests and the offline Android
unit build, but did not run emulator instrumentation.

The signed stale-epoch receipt now binds to its historical rotation prefix
even after later accepted history, while rebatching uses the latest verified
sequence. A saved pre-commit claim reopens by Family/device identity and
retries its exact bytes without the fragment. The recheck requested an
Activity-recreation regression, a forced recipient background-job regression,
and correction of BATCH04's scenario label. Activity recreation was already
covered in `164f0ff`; the job regression and vector label are added in the
follow-up slice. The real-relay emulator runner passed all six Android
instrumentation tests, including the forced recipient job. General-claim
rebase after unrelated control changes remains
a blocker before later-device enrollment. Full rotation, removal, automatic
private copy, and malicious-relay withholding limits remain outside this
focused pass.

## Advisory Android sync review at 6b10805

Daybreak Blue at high thinking reviewed fixed commit
`6b108056b8407a61f8ecf27373318053aa229296` through Yep Anywhere
session `01a0e0b2-c3e9-7ef2-82b3-7c83a9622525` in a read-only separate
checkout. The reviewer ran offline core, CLI, and relay tests, inspected the
Android test without running an emulator, and verified the checkout stayed
at its target. Result: **advisory FAIL** with two high blockers and one
medium visibility finding. This is not the early M0 security gate.

| Finding | Disposition and evidence |
|---|---|
| A signed stale-epoch or sequence-conflict rejection could strand the Android outbox indefinitely after an uncertain POST. | `217e0bd` adds a signed exact batch-result query, verifies the accepted/rejected identity in Rust, and rebases only a supported rejection after the required verified prefix. A CLI competing-sequence regression and an emulator dropped-acceptance test pass. Unsupported rejections retain work and remain a UI/recovery gap. |
| Enrollment continuation depended on the invitation fragment in UI memory, so a process restart could strand a recipient. | `3e6e84d` enumerates the durable recipient attempts and reuses their pinned relay origin. The emulator resumes by Family ID after reopening the coordinator. A full Activity force-stop/recreation test remains open. |
| Authorized hostile batches become inert in the core, but the Android view hid the integrity gap. | The shared snapshot now carries inert count and recent reasons from verified replay, and both manager and recipient views show a history-gap warning. The core unopenable-batch continuation test passes; a real-relay Android display regression remains open. |

The review assumed an honest ordered relay for the principal flow and did
not claim protection from malicious relay withholding/forking. These fixes
need a new fixed-revision follow-up before the advisory can be closed.
General later-device enrollment, removal and epoch rotation, automatic
private copy, and suspended-app delivery remain outside this preflight.

## Early M0 implemented-authority review at 2507537

The independent Daybreak Blue high-thinking session
`01a0e102-c203-7c23-aa5d-c1af34088948` reviewed fixed commit
`250753787f12cbfad3eda7dcfee6fea55f7ca8fe` in a detached read-only
checkout through Yep Anywhere. The reviewer ran offline core, server, and
real-relay CLI tests (56, 10, and 2 passed). The verdict was **FAIL**;
the early M0 gate stays open. The reviewed SHA precedes the Android removal
UI and the removed-device proof/copy implementation.

| Finding | Disposition |
|---|---|
| Medium blocker: the relay denies a removed author its own accepted or rejected batch result after a lost POST response, so pending-work disposition cannot be verified. | The relay now allows only the removed signer's accepted receipt and verifies authorship of stored rejected envelopes before returning those receipts. FS63/BATCH08 and a real-relay CLI lost-accepted-response variant cover accepted, rejected, unknown, known-other, and denied post-removal data. Android queries the saved batch result after proof and reports accepted, rejected, or unknown while preserving local work. The fixed-revision recheck at `82c0f4c` passed this repair. |
| Medium hardening: protocol IDs can be reused across categories even though the contract says never reused. | The first-cohort relay now scans its durable committed control and accepted/rejected result registry inside each commit transaction; the core replay keeps a global Family ID set. FS64/IDCOLL01, core cross-category cases, and relay registry checks cover selected collisions. Extend exact signed collision tests and the registry as later-device authority is implemented. |
| Low contract gap: three advertised GET routes are not implemented. | The relay now implements authenticated `batches?after`, `control-results/{id}`, and first-cohort `invites/{id}`. APIROUTES01 and the real-relay CLI exercise committed, unknown, and removed batch-page cases. Broader unauthorized and mixed-client route coverage remains. |

The reviewer found no present two-device privilege escalation from the ID
collision, and confirmed signed CAS, key rotation, verified grant/keyring,
revoked data ACL, and epoch-two manager continuation in the reviewed path.
These positive findings do not close the gate while the receipt blocker
remains. Android/emulator, fuzzing, and power-loss campaigns were outside
this review.

First-cohort ID hardening now rejects a new transition, object/grant,
invitation, claim device, challenge, or accepted batch ID reused across
categories in Rust replay. The relay scans durable committed controls and
accepted/rejected batch results inside each commit transaction, including
after restart. Core collision tests and relay registry assertions pass;
IDCOLL01/FS64 records the remaining signed stage/commit combinations to
exercise as later-device authority expands. The disposable-relay Android
suite still passes six tests after this guard.
The three previously advertised GET forms now have relay handlers. Filtered
batch pages keep global cursor numbers while omitting controls, and only
active readers may fetch them. Signed public control-result lookup returns
exact committed bytes or unknown; the first-cohort invitation route returns
the committed issue to its invitation signer or an active device. The
real-relay CLI covers these paths and removed batch-page denial.

## Early M0 authority recheck at fd4aca4

Daybreak Blue high thinking reviewed fixed commit
`fd4aca4a2053c9705bbcf21941d4e46d31134534` in a detached checkout,
session `01a0e11f-8d82-76b3-87bc-18dc15800975`. The checkout stayed
clean. The reviewer ran 56 core, 10 relay, 4 CLI integration, and 2 Android
JVM tests offline; no emulator or fault campaign. Verdict: **FAIL** for one
medium malicious-relay cutover blocker. The earlier honest-relay removed
author result ACL blocker passed. The fixed SHA preceded the ID registry
and three GET-route commits, which have since received local regressions.

The reviewer showed that a relay-signed acceptance for the exact pending
envelope at or after verified removal could be displayed as accepted
before removal. A contradictory rejected receipt could also be trusted.
Classification now requires a persisted verified removal, checks the exact
signed pending envelope and receipt fields, bounds acceptance strictly
between the old pin and removal cursor, and requires a post-cutover revoked
rejection with the verified removal head. Contradictions remain unknown;
the source outbox and private copy persist. FS63/BATCH08 and a real-relay
CLI test inject signed inconsistent cursors and heads, including after
SQLite reopen. The fixed-revision independent recheck at `82c0f4c`
passed this repair.

A new `scripts/check_android_two_emulators.sh` drives a disposable relay
and ten instrumentation invocations across two separately stored emulator
installations. It passed local manager promotion and invitation, recipient
claim, holder challenge, recipient proof, admission, reciprocal child
uploads, offline staged edit, manager removal, recipient signed proof,
unknown result after restart, read-only source, and idempotent private
copy. This exercises separate
Android Keystores. It is emulator evidence; the M1 two-physical-phone gate
remains open. The required CI emulator job still runs the focused
single-emulator `SharingRelayTest` suite.

## Early M0 authority recheck at 82c0f4c

Daybreak Blue high thinking reviewed fixed commit
`82c0f4ca4950937947d00e44d60ab76cd9472fc3` in a detached clean
checkout, session `01a0e13f-1942-74a2-8b67-c9eae7036f7e`. It returned
**PASS for the implemented first cohort**: one initial manager, one
admitted recipient, and first recipient removal. The reviewer found no
remaining access or pending-work blocker. It ran focused core (52), relay
(10), CLI (4 integration), and Android JVM (2) tests. It inspected but did
not run the two-emulator suite, physical phones, full workspace, Clippy,
fuzzing, or power-cut campaign.

The reviewer verified that the saved removal proof and exact pending
envelope bound accepted and rejected status to the cutover. Cross-category
IDs cannot commit twice in the first cohort, and the three authenticated
GET routes have no identified first-cohort ACL bypass. A malicious relay
can still withhold or fork a branch-consistent history, as documented in
the trust model; a receipt is not globally witnessed inclusion.

| Low finding | Disposition |
|---|---|
| A revoked rejection with arbitrary `next_expected_sequence` could still be labeled rejected. | The classifier now requires the pending sequence for the implemented first cohort. FS63/BATCH08 and the signed real-relay CLI receipt variant cover a changed value before and after SQLite reopen; inconsistent evidence remains unknown. |
| FS64 overpromised staging and client detection for IDs only present in private rejected-batch results. | The protocol and FS64/IDCOLL01 now state the commit-time guarantee, that staging can temporarily accept, and that client replay detects only public-chain collisions. The relay keeps rejected IDs reserved durably. Signed full collision-matrix route coverage remains M0 work. |

This focused PASS clears the first-cohort early authority review, not the
M0 exit review or later-device authority. The M1 two-physical-phone and
power-loss gates remain open.

## Advisory general-authority seam review at bb68156

Daybreak Blue high thinking reviewed fixed clean commit
`bb681565a40c85c5de6cced27650788222195d76` through Yep Anywhere in
read-only plan session `01a0e187-dfc1-7160-aa99-1efb5da63b81`. It returned
**FAIL for the proposed general relay-authority implementation seam**, not
for the reviewed first-cohort gate or the M0 exit gate. It modeled the
honest ordered relay, hostile authorized device, compromised storage, and
malicious relay within the documented trust limits. This was a static review;
it ran no tests, build, lint, emulator, browser, fuzz, or fault campaign.

| Finding | Required implementation and regression |
|---|---|
| High: candidate verification cannot decide expiry from a timestamp captured before the transaction. | Split deterministic candidate preparation from commit-time checks; choose signed time inside the write transaction and test the exact expiry boundary after delayed staging and restart (FS65). |
| High: current authorization state discards removed credentials needed for narrow historical reads. | Rebuild a public history ledger and test every route for invitation, pending, active, and removed devices, including own-result access and other-device denial (FS66). |
| High: batch checks need historical head/epoch, sequences, and IDs. | Include these in the public ledger and test old-head/wrong-epoch, same-epoch ancestor, competing sequence, rotation races, and restart (FS67). |
| High: singleton Family staging can let an incomplete hostile candidate block another manager. | Key staging by candidate; test two same-head proposals, incomplete A, committed B, then stale A after restart (FS68). |
| High: current authority checks before exact retry lookup can lose a committed result after role loss/removal. | Look up exact bytes and return the original result first; test later controls, removal, restart, and same ID with different bytes (FS69). |
| High: control-head-only CAS does not serialize batches, which advance the global cursor. | CAS head and cursor with all derived acceptance state in one SQLite transaction; test separate connections, control/batch races, and crash before/after commit (FS70). |
| Medium hardening: hash/size checks alone allow public grant envelopes with wrong recipients or key versions. | Validate public envelope shape and bindings without opening ciphertext; add wrong/duplicate/removed recipient and wrong purpose/version cases. |
| Medium hardening: unchecked cached authority rows can turn writable SQLite corruption into signed invalid decisions. | Rebuild or verify the ledger from signed history on restart; independently corrupt derived rows and require recovery or fail closed. |

The first-cohort claim now opens an immediate SQLite write transaction before
sampling the relay clock for a new claim. Its expiry check and signed receipt
use that in-transaction time; an exact committed retry returns its prior
result without resampling. The server fixture test proves the clock callback
runs while another connection cannot acquire the write lock, rejects exactly
at expiry without advancing cursor/head, and preserves the published claim
bytes and retry. FS65's later-device, restart, and interleaved-batch variants
remain open. Other general-authority findings in this table remain open.

The first-cohort batch path now parses only the claimed ID and author before
acquiring an immediate SQLite write transaction. It returns a byte-identical
accepted or rejected result before consulting current roles, then validates
new work against one transaction snapshot. Accepted batches condition their
cursor update on both the old cursor and control head and reject a failed
update. Two separate relay connections racing the same signed envelope get
one log entry and the same result; the HTTP retry agrees. This covers the
exact-retry and same-envelope cursor portion of FS69-FS70. Distinct control
versus batch and rotation races, later-role-loss retries, and crash points
remain open for general authority. The five first-cohort post-genesis
control commits now acquire that same immediate write transaction before
reading control history, head, or cursor. A separate-connection race between
a signed initial-manager batch and invitation issue produces exactly one of
each log entry at consecutive cursors. This covers the first-cohort
control-versus-batch part of FS70; general rotation and crash points remain.

First-cohort control objects now stage under their signed transition ID.
An interrupted single-slot relay database migrates its staged candidate on
open. A regression stages a different signed issue proposal, restarts,
stages and commits the valid fixture proposal, then rejects the stale one
after another restart; committing one candidate clears the competing staged
rows. This covers the candidate-isolation and restart part of FS68. The
multi-object incomplete-A case and general later-device contenders remain
open with the broader public-authority work.

The first shared public candidate verifier for invitation issue now lives
in `wire/` and is called by both the client control chain and the relay.
The reviewed first-issue fixture, signed competing issue, core control
chain, server, and real-relay flow remain byte-identical. It has no private
key or relay clock input. This is a first extraction only: historical IDs,
all other transition kinds, the rebuildable public ledger, and general
later-device routes remain to be factored and tested.

The same verifier now prepares two-signature invitation claims from verified
public state. Both the client and relay use it for the invitation/device
signatures, claim transcript, and pending state effect. Historical device and
transition IDs, relay receipt, and commit-time expiry remain at the callers.
The first-cohort relay now rejects invitation and claim IDs colliding with
genesis or issue IDs across categories, matching the client history check.
The control-chain and relay fixtures pass; later-device claims still need the
general public ledger and route.

The shared `wire` verifier now checks the common signed envelope for the
remaining control kinds on both sides: Family/relay/head/epoch context,
state and core hashes, manifest sizes and order, and signer signatures.
Transition-specific next-state and signer selection still live in the
respective callers. This removes duplicate envelope validation without
claiming a complete common authority reducer; the public ledger and later
device behavior remain open.

Holder challenge is the next shared state effect: the verified pending row
supplies the device, key version, claim hash, and challenge context; both
client and relay derive its next public state in `wire`. The client still
checks historical IDs before candidate preparation and retains receipt and
object verification. Other transition state effects remain to be extracted.

Pending key proof now uses shared state preparation as well: its signed
device claim must match the latest verified challenge ID and hash, and both
sides compute the same proof hash and pending-state effect. The separate
private challenge-object opening remains a client concern. Admission and
later controls still need shared state reducers.

Admission now uses the shared reducer: the fixed role and proof on the
pending row, current epoch commitment, signer, active-row insertion, and
grant manifest are checked from public state on both client and relay.
Encrypted grant and membership object contents remain client checks, and
historical ID checks remain separate. Repair, cancellation, role change,
pending removal, and later-device relay routes remain open.

Active removal now uses shared public preparation too. It checks a manager
signature, target and prior role, a changed epoch commitment, a surviving
manager, canceled unused invitations from the removed issuer, cleared pending
proofs, and the rotation manifest shape. The client still opens and verifies
the new grants and keyring. General relay rotation and historical access
remain open; the current route still covers the first cohort only.

Unused-invitation cancellation, active role change, and pending-device
removal now derive their state effects and active-manager signer policy in
the shared verifier. The client control chain uses those preparations and
the fixed control-chain fixture covers the three accepted transitions,
last-manager rejection, and cancellation by a manager other than the issuer.
The relay has no general routes for them yet. Grant repair now uses a shared
reducer against the client's verified admission association and epoch
commitment; the relay still needs a durable historical admission and
commitment ledger before it can use that reducer.

The corrected [topic proposal](../topics/sync-and-encryption.md#proposed-implementation-seam-for-general-relay-authority)
uses a public candidate verifier, a relay commit wrapper, and a client
committed verifier. It needs no v1 wire change if the ledger remains internal;
first-cohort bytes and ACLs are regression anchors. The reviewer found no
private-key dependency in this factoring. A malicious relay can still withhold
or fork valid history or lie about time, and a legitimate holder can disclose
keys it possesses. General authority and the M0 exit gate remain open.

## Advisory byte/crypto preflight

The independent Daybreak Blue high-thinking read-only session
`01a0df22-146a-7081-ae56-e715e47aad23` reviewed fixed commit `8402bd8`
through Yep Anywhere. The checkout advanced during the review; the reviewer
re-verified cited source at that SHA with `git show`. It assumed the agreed
honest-relay order and separately considered a hostile authorized device,
compromised relay storage, and malicious withholding/forking. It did not run
tests; its request to execute Cargo tests was denied to keep the review
read-only. This optional preflight returned **advisory FAIL**, not a named
M0 gate result.

| Finding | Disposition and required regression |
|---|---|
| High: platform bindings accepted caller-supplied signer, key, relay, and cursor, including an unchecked control advance. | Primitive exports are now fixture-only; the production Rust session must verify committed control, receipt, epoch, sequence, and author before projection. Bind FS50 stale batch after removal and FS56 rollback/sibling cases. |
| High: sealing accepted caller-owned nonce, batch ID, and sequence without durable retry identity. | Fixture sealing is excluded from production bindings. Native SQLite now stages random ID/nonce and exact bytes atomically, retains reservations, and retries identical bytes after restart. A core-owned session and browser outbox must verify acceptance before clearing or rebatching. Bind BATCH01/02/04 crash/retry cases. |
| Medium: primitive sealing could sign a structurally valid but locally invalid edit. | Production `append_and_prepare` must validate against local projection before allocating HLC, nonce, or sequence. Incoming hostile malformed batches remain inert. Bind PRECREATEBYTE01 and a conflicting-pump local/incoming pair. |
| High: a validly signed but undecryptable batch from an authorized hostile writer could stop replay before inert classification. | Signature-checked envelope replay now marks AEAD failure inert only when given a core-verified committed epoch key. `UNOPENABLEBYTE01` exercises signature-valid/AEAD-invalid bytes and continuation; the production authority session must construct the verified key from committed control. |
| Medium: PRECREATEBYTE01 used different child IDs for its create and invalid set, leaving set-before-create untested. | Corrected the rollback assertion to inspect the created child and added `SETTHENCREATEBYTE01` across Rust, Swift, Kotlin, and wasm. |
| Hardening: full projection clones per batch; unbounded host response and GC-managed key-copy risks. | Address before real data with FS60 work-bound stress, bounded transport reads, and platform key-storage review. |

The reviewer found no isolated CBOR, domain-separation, signature, AEAD,
fixed HPKE receiver, or unknown-field defect at the reviewed SHA. The early
implemented-authority gate and end-of-M0 gate remain open. The fixture-only
boundary removes one exposure but does not resolve the durable session and
outbox findings.

## Advisory shared authority verifier at 7566262

Daybreak Blue at high thinking reviewed fixed clean commit
`7566262f2996e8b3c718cc20b42c851e0997d4a2` in read-only Yep Anywhere
session `01a0e1f0-a10c-7611-a36d-ef3613b80ec1`. The process reached
verified idle with an empty queue. The static review ran no tests and returned
**focused advisory PASS** for public state agreement on the currently exposed
first-cohort issue, claim, challenge, proof, admission, and removal routes,
assuming an honest ordered relay and intact storage. This is not the end-M0
gate or a pass for later-device authority.

| Finding | Disposition and required regression |
|---|---|
| G1 high: the common signed envelope checked caller-supplied state and signer policy. | Shared reducers derive cancellation, role change, pending removal, and repair effects and signers for client replay. A relay public ledger now replays all published control kinds through those same reducers at restart and matches the fixed chain's state and head. General stage/commit routes still must use that authenticated ledger inside their transactions; differential negative cases for stale or member signers, last-manager loss, wrong pending target, and wrong repair admission remain (FS71). The review suggested rejecting cancellation by a non-issuer, but agreed D8 allows any active manager to cancel an unused invite; FS71 tests that valid case. |
| G2 high: the public ledger proposal did not explicitly bind every epoch to its verified commitment. | The shared wire crate now enforces immutable head-to-epoch and epoch-to-commitment bindings for client replay and relay restart reconstruction. The relay rejects a stale or substituted accepted-batch authoring head, and the two-emulator flow restarts the relay after rotation before the removed recipient checks access. General route candidate validation must use this ledger inside the commit transaction; later-device admission and FS72 negative cases remain a blocker. |
| H1 medium: staging reserved object IDs only within a candidate; cleanup could allow reuse of an uncommitted ID with different bytes. | The relay now retains a Family-wide object-ID-to-hash reservation independently of candidate cleanup and migrates existing staged/committed rows, failing startup on conflicting bindings. A competing signed first-issue stage is rejected after cleanup and restart. Extend FS73 to cross-category stages before closing the full general route gate. |
| H2 medium: first-cohort restart reconstruction did not authenticate every mutable stored authority link and receipt. | Active genesis reconstruction requires the saved candidate and committed Family row to match the signed cursor-one log entry and verifies the relay receipt before first-cohort reads or commits. On reopen, the relay checks contiguous log cursors, every control receipt and parent-head link, all public control transitions through shared reducers, committed objects against signed manifests, the saved head/cursor, and accepted batch result bytes and metadata against the log. It also rechecks each accepted batch's historical active signer, signature, sequence, unique ID, head ancestry, and epoch. Restart tests reject a different validly signed genesis candidate, altered controls or batch receipts, altered objects, a changed head, a relay-resigned pending-device batch, and a relay-resigned sequence gap. Extant rejected results and their completeness now have a signed private checkpoint; historical-read and other derived-row checks remain (FS74). |
| H3 medium: admission staging checked grant hash and size but not all public grant-envelope bindings. | First-cohort admission staging and commit now validate grant ID, recipient, purpose, key version, suite, and control-core hash through the shared public checker. The published grant fixture and malformed-envelope cases pass; opaque but structurally valid ciphertext remains a client readiness check. Extend this rule to general admission and repair (FS71). |

The reviewer found no accepted first-cohort candidate that makes an honest
client and relay derive different public state. It accepted the documented
limits of authorized hostile holders, withheld/forked history, relay-controlled
time, and no global fork detection. The [owning topic](../topics/sync-and-encryption.md#proposed-implementation-seam-for-general-relay-authority)
now states the additional ledger invariants. The general relay and formal
M0 exit review remain open.

A later internal relay commit primitive now uses that reconstructed ledger
under one SQLite writer transaction for object-free manager controls 3, 7,
and 9. A signed unused-invitation cancellation passes exact retry, restart,
stale-head, backward-clock, interleaved data batch, and same-ID/different-bytes
checks. That
regression found the shared reducer and client replay wrongly demanded one
manifest object for these three kinds; the published contract requires none,
and both now agree. HTTP exposure, later-device tests, and historical reader/batch
authorization still need implementation.

The relay's signed GET boundary now rebuilds the same ledger for reader
classification instead of inferring current roles and removal from seven
fixed control positions. Published first-cohort states assert manager,
unused invitation, pending challenge, admitted member, and removed-device
classes; the dynamic CLI and two-emulator flows exercise the HTTP paths.
General later-device negative and post-rotation read cases are still needed
before closing the reader authorization gate.

The relay batch POST now derives the author's signing key and active status,
the current epoch, historical head binding, and next sequence from the
authenticated public ledger in the same SQLite writer transaction. A durable
manager batch with an earlier same-epoch head commits after a cancellation;
exact retry returns the saved receipt before and after restart. Workspace
tests and the two-emulator join, bidirectional sync, removal, and private-copy
flow pass. The same verified no-object manager path now serves signed HTTP
cancellation, role-change, and pending-removal requests. Later-device and
rotation-race signed cases remain before the general authority gate closes.

An internal general control writer now stages signed manifest objects and
commits later issue, claim, challenge, proof, admission, repair, and rotation
through the same ledger inside one SQLite write transaction. The published
control chain and interleaved batch reproduce exact bytes, retry exactly,
and survive reopen. A missing or altered staged object fails before commit.
The HTTP boundary now routes later control kinds to that transactional writer
while retaining the stricter first-cohort paths at their original positions.
Public admission, repair, and rotation grant envelopes must name the right
recipient, key version, purpose, and transition core; rotation covers each
remaining active device exactly once. A later repair and rotating removal
reproduce published bytes through HTTP and reopen cleanly. General negative
role and epoch cases, Android later-device preparation, and a focused
independent review remain.
FS67 now has a signed relay regression after rotation: an old-epoch upload
and a new-epoch upload naming an old head receive distinct signed rejections;
the manager's next sequence with the new epoch and removal head commits and
survives restart. Competing writers and a later-device role loss remain.
A fresh signed manager invitation with a correctly encrypted membership
object now stages and commits after rotation and data upload. Its unused
invitation credential appears in the replayed reader state, proving the
writer is no longer tied to the original join positions. A second recipient's
signed claim now commits, changes that invitation to consumed, and appears
only as a pending reader. Its signed pre-admission data upload gets a revoked
receipt. The initial manager can now prepare and confirm a challenge for a
later verified claim at its current data-ready head. The Rust path targets
the named pending row, encrypts the verifier under the current epoch key,
resumes exact bytes after restart, and can release an earlier committed
preparation slot when a new challenge is needed. Real-relay tests stage and
commit both challenge objects after unrelated issues, and again after the
first recipient's removal and epoch rotation to admit a new candidate.
The later recipient now verifies that challenge and its HPKE object, prepares
an exact signed proof, resumes it after restart, and commits it through relay
HTTP. The manager selects the proved pending row, encrypts the admission
membership with the current epoch key, and grants that key to the recipient.
The recipient verifies its admission, replays the contiguous Family log,
downloads authenticated objects, and becomes data-ready. Real-relay tests
cover both epoch one and admission after removal and rotation to epoch two.
Android now uses verified pending state to choose a manager action and can
issue a later invitation. A separate-emulator relay flow creates and removes
the first recipient, restarts the relay, then takes a new device through a
later invitation, claim, challenge, proof, grant, hydration, encrypted upload,
and manager readback at epoch two. A second manager can now issue a later
invitation, challenge and grant a third device using its own credential and
verified epoch key. The CLI proves that path through relay HTTP; Android's
real-relay instrumentation covers the same three-device handoff and a later
encrypted edit. Prepared-control replay now checks the saved sparse join
prefix and then the fully verified log, since a manager can act after its
own enrollment. The admitted manager can also remove that third device at
epoch one, granting the rotated key to remaining holders. The CLI checks
the removed device's new-data read denial; Android checks the removed state,
hides its stale shared snapshot, and confirms the original manager still
syncs. An Android relay case now removes the original manager in a second
rotation and continues encrypted writes at epoch three. Admitted-manager
cancellation and broader recovery cases remain open. The removed original
manager can now fetch a signed public removal proof and save an offline edit
into one private copy; the three-device Android relay case verifies this.
The Android tracker excludes that source from writable Family selection,
offers an access-ended copy action, and reuses the saved removal copy. A
focused UI tap check of that card remains open.
Ready replay can now start from a later recipient's admission grant at a
rotated epoch: it verifies that epoch's committed keyring and membership,
recovers earlier keys, then applies subsequent addressed rotations. A fixed
rotation fixture proves known-key history replay without an impossible old
rotation grant and rejects a tampered keyring or key. The admitted-manager
third-device case now covers admission and hydration at epoch one; later
rotations and recovery variants remain open (FS72).

The relay page selector now stops before the four-MiB encoded response cap
instead of blindly taking 256 entries and failing the entire read. FS75
pages through both canonical large entries and 25 signed near-limit batches
via authenticated full and filtered reads, including after relay restart;
the two-emulator sync flow still
passes. A bounded transport HTTP variant remains for the M0 mixed-client
gate.

## Advisory general-authority implementation review at 2461763

Daybreak Blue at high thinking reviewed fixed clean commit
`24617639ba5bef9bef7a98bbf488b7cad51150d4` in read-only Yep Anywhere
session `01a0e283-9244-7262-9f89-2217b987de9e`. It reached verified idle
with an empty queue and made no edits. The reviewer ran server, dynamic CLI,
and core control-chain tests. Result: **advisory FAIL**, not the formal M0
exit gate. It found no concrete role escalation, forged batch authorship,
stale epoch/head/sequence acceptance, premature grant exposure, partial
control commit, or first-cohort retry regression.

| Finding | Disposition |
|---|---|
| High: 17 valid near-limit batches could make `/log` and `/batches` permanently unpageable at the four-MiB response limit. | The relay now takes the largest fitting contiguous prefix and returns `has_more`. FS75/APIPAGE02 exercises 25 signed large batches through authenticated full and filtered pages after restart. Large-control and later-device hydration variants remain for the mixed-client gate. |
| Medium: the exact protocol required a membership object for cancellation, role change, and pending removal while implemented client/relay reducers required empty manifests. | The exact v1 contract now has a per-kind object table. The three object-free manager controls use empty signed manifests, and deterministic full committed bytes are pinned in `no-object-controls-v1.json` and checked by the core control-chain test (FS71). V1 is undeployed, so no migration is required. |
| Medium hardening: staged object reservations did not participate in the Family-wide public ID collision check. | The relay checks permanent reservations when committing public IDs, permits a candidate's own exact object binding, and checks committed-object consistency. FS73/IDCOLL01 now names batch, transition, device, and invitation collisions; a signed colliding batch fails before and after restart while the original object remains restageable. Other category variants remain for later-device testing. |

The reviewer assumed one honest ordered relay process with SQLite writer
transactions, authorized hostile devices, and storage compromise without
the live relay signing seed. Malicious relay forks, withheld history,
signed-time lies, and coherent whole-database rollback remain the documented
limits. A fresh fixed-revision recheck is required before this advisory can
pass; full M0 exit still depends on the broader mixed-client and recovery
gate.

## Advisory public-ledger restart review at bdbed9e

Daybreak Blue at high thinking reviewed fixed clean commit
`bdbed9e23a1a7226940f5e580d6603a822d0b5c9` in read-only Yep Anywhere
session `01a0e246-2f44-7850-a10f-e2b7a3f2f282`. The process reached
verified idle with an empty queue; the reviewer made no edits and ran no
tests. Result: **focused first-cohort PASS**, not the M0 exit gate or approval
of later-device authority. The reviewer found no signed first-cohort control
that makes honest client and relay public state diverge. The relay rebuilds
one contiguous, internally consistent signed branch, not a globally unique
history against coherent rollback or a malicious relay fork.

| Finding | Disposition and required regression |
|---|---|
| Medium: rejected-only result rows were outside restart authentication. A storage attacker could delete or substitute one, changing exact retry behavior and freeing its private ID reservation. | Restart authenticates each extant rejection's relay signature, envelope hash, ID, sequence, Family/relay, and historical cursor/head. A relay-private signed checkpoint now binds the complete rejection set. Altered and deleted rows fail reopen in focused tests (FS62, FS64, FS74). Exact retry after removal and cross-category ID reuse still need adversarial regressions. Replaying a saved older checkpoint with matching private rows remains possible without an external witness. |
| Medium: a staged object reservation can be the only remaining trace of an object ID/hash after staged bytes are cleaned. Deleting that row allows the same ID with a different hash after restart. | The same signed checkpoint binds the complete permanent reservation set. A reservation-only deletion after candidate cleanup fails reopen (FS73); cross-kind and rollback-snapshot variants remain. Nonempty pre-checkpoint development databases require explicit migration after operator inspection; migration cannot prove that earlier private rows were complete. |

Before exposing general authority, derive historical reader credentials,
batch author/sequence status, all public IDs, and admission associations from
the authenticated ledger inside commit transactions. Restart now rechecks
accepted batch author, signature, sequence, head, epoch, and ID against that
ledger; exercise FS66–FS74 with later-device and crash cases.
The reviewer accepted the documented malicious-relay fork, signed-clock, and
stale-epoch writer limits. Local Rust, bindings, browser, and two-emulator
validation for subsequent commits is separate evidence and does not change
the reviewed SHA or this review verdict.

## General-authority advisory recheck at f55289a

The first plan-mode recheck was canceled at the user's request and has no
verdict. Daybreak Blue then reviewed clean fixed commit
`f55289abe3ff3931bd8207fe0eb1945ff0a6429e` in a disposable detached
checkout, high thinking, bypass-permissions mode, read only, through Yep
Anywhere session `01a0e2ae-044d-72a2-acc9-90e018b610f5`. It reached
verified idle with no source edits. Result: **focused advisory FAIL**.
The pagination and object-manifest findings were closed. The remaining
medium availability finding was a batch-first ID collision: after an
accepted or rejected batch used ID X, a valid manager could stage a
membership object with ID X. Staging permanently reserved X, issue commit
rejected, and subsequent unrelated Family writes failed because public
and private ID registries now conflicted.

The relay now rejects a newly staged object ID already present in the
authenticated public control and accepted/rejected batch registry before
creating a reservation. Restart rejects an existing uncommitted reservation
that conflicts with that registry. FS73/IDCOLL01 and the executable signed
relay regression cover both accepted and rejected batch-first orderings,
absence of a poison reservation, restart, and subsequent unrelated signed
batch and valid control commits.

## General-authority advisory PASS at 98fa0ea

Daybreak Blue reviewed clean fixed commit
`98fa0ea078764f938cf192deb4573e7b1feef2a7` in detached checkout,
high thinking and bypass-permissions mode, through Yep Anywhere session
`01a0e2be-f107-7be1-b4bd-541c111cde3a`. The process reached verified
idle and left the checkout unchanged. Result: **focused advisory PASS**.
It found no substantive security or availability issue in the batch-first
reservation repair. It confirmed accepted and rejected batch-first order,
transaction rollback of failed staging, valid unrelated batch and control
after restart, and no regression of page bounding or the v1 per-kind
manifest rules. It ran `cargo test -p babytrack-server --locked` (23 tests),
three focused server tests, `cargo test -p babytrack-core --locked --test
control_chain` (7 tests), and `cargo test -p babytrack-cli --locked`
(4 integration tests). The signed large-control page and later-device
hydration variants were inspected but not exercised. A relay already
poisoned by the earlier undeployed build fails startup and needs inspected
operator recovery; it is not automatically healed. Coherent whole-database
rollback, malicious relay withholding/forks, and signed-time lies remain
the documented trust limits. This scoped PASS does not close the end-of-M0
recovery, mixed-client, or physical-device gates.

## Advisory later-invitation and paged-claim review at a6ef118

Daybreak Blue at high thinking reviewed clean fixed commit
`a6ef1189c439dbfbbf117db21ad36ea3cde32fcf` in a disposable detached
checkout, read only, through Yep Anywhere session
`01a0e30d-8869-7542-954c-ff5d976496f1`. The process reached verified
idle with an empty queue and did not edit the checkout. Result: **focused
advisory FAIL**, not the M0 exit verdict. The reviewer assumed an honest
ordered relay for availability and also checked hostile authorized input,
withholding, signed history, and the agreed malicious-relay limits. It ran
four dynamic CLI tests, core enrollment/control-chain tests, focused server
tests, and an FFI build; it did not run Android.

| Finding | Disposition |
|---|---|
| High: an unrelated control after a later invitation issue made sparse recipient preparation require the issue as the final control. A control after saved claim preparation left Android retrying stale exact bytes forever. | FS76 and the dynamic real-relay test now cover controls both before and after saved preparation, page splitting, restart, rebase with stable keys and nonce, and reconciliation of an accepted claim after a lost response. Rust verifies the complete sparse prefix, archives superseded exact candidates, and Android refreshes before retry. A fixed-revision independent recheck remains due. |
| Medium: cancel, expiry, or consumption closes the invitation read credential with unsigned HTTP 403, so a recipient cannot verify a terminal reason against a malicious relay. | FS77/INVSTAT01 define a narrowly scoped relay-signed status response, available through the historical invitation key after ordinary reads close. Rust checks the link binding and signature; Android shows a terminal reason only after that check, otherwise reports unknown status. Core vector, relay cancellation/expiry, and CLI consumed-claim tests pass locally. Independent recheck remains due. |
| Medium hardening: Android/FFI stop after 64 control pages or 16 MiB, leaving sufficiently long valid control history unjoinable. | Streaming, resumable verification and a large-history regression remain open. The current bound is reported as an implementation limit, not a protocol limit. |

The reviewer also confirmed link binding, control signatures and cursors,
relay writer compare-and-swap, and keyless recipient isolation. Those
observations do not close the open findings or the broader M0 gate.
The signed-status repair also passed the two-emulator first-cohort relay
flow; that flow does not force the terminal-status UI branch.

## Advisory later-invitation recheck at 137e1b1

Daybreak Blue at high thinking reviewed clean fixed commit
`137e1b1e3b502bffbd1f3a2481bb0d7cf202156d` in a detached read-only
checkout through Yep Anywhere session
`01a0e332-11eb-78d1-8321-9f7285245e9d`. It reached verified idle, with
an empty queue and no checkout edits. Result: **focused advisory FAIL**.
The reviewer ran full core (72 tests), server (23 tests), CLI (5 integration
tests), FFI check, and focused cases; it did not run Android. It accepted
the sparse-prefix verification, stable device keys and nonce, archived exact
candidate reconciliation, and status route's narrow read ACL. It found no
forged authority or Family-data disclosure. A malicious relay's withholding,
fork, signed-time lies, and coherent rollback remain documented limits.

| Finding | Disposition |
|---|---|
| High: automatic recipient progress inferred claim/proof from the absolute count of sparse controls. A later issue with preceding controls could leave a saved uncommitted claim waiting forever after restart. | FS76's real-relay CLI case now asserts the next action before and after later controls, restart, rebase, and accepted claim. Rust derives the action from verified invitation and pending state. Android still needs a later-device coordinator regression. Fixed-revision review due. |
| Medium: the Android join screen discarded verified terminal errors, and expiry could be discovered only at claim POST rather than the earlier read. | The coordinator now queries signed status after any claim failure and returns a typed terminal reason. The screen has distinct claimed, canceled, expired, and issuer-invalid text; unsigned denial and unavailable status remain retryable unknown. The Android real-relay test confirms a second device receives the verified claimed reason; other terminal UI fault cases remain. Fixed-revision review due. |
| Medium: issuer demotion/removal and explicit cancel all wrote invitation state `3`, so status reason `5` was unreachable; a later control could overwrite earlier expiry. | The relay now reconstructs the first terminal cause from verified transition kind and signed commit time, with expiry winning at or before that control. FS77's cause tests cover cancel versus issuer demotion/removal, expiry, and no revival in the reducer. A full end-to-end demotion/removal status test remains due. Fixed-revision review due. |
| Known hardening: 64-page/16-MiB Android join history ceiling. | Still open for durable streaming and resume; excluded from this focused verdict but required before the unbounded history claim closes. |

## Advisory later-invitation automatic-status recheck at 3f19890

Daybreak Blue at high thinking reviewed clean fixed commit
`3f19890c774272c2db04f478cbd55eaa5614d04c` in a detached read-only
checkout through Yep Anywhere session
`01a0e34c-0d55-7302-bb5d-b8ab05e00a3b`. It reached verified idle with
an empty queue and no checkout edits. Result: **focused advisory FAIL** on
one remaining medium Android automatic-status path. Core and relay repairs
passed inspection: automatic action uses verified authority state, and the
first terminal cause survives relay restart. The reviewer ran core (77
tests), server (23), CLI (5 integration), Kotlin and Swift native smoke,
Android JVM tests/build, and seven Android real-relay instrumentation cases.
It did not exercise a terminal UI branch or later-device automatic retry in
Android; neither was inferred from a passing adjacent test.

The foreground poller discarded a typed `InvitationTerminal` and showed only
generic sync delay; the background job recorded only a failure bit. A saved
invitation that became unusable therefore needed a manual retry to reveal
its already verified terminal reason. The repair adds a v4 local-store table
for the exact signed status body, re-verifies it on reopen through the shared
core, and returns the typed reason at the start of automatic recipient work.
Foreground polling opens the join status and renders its distinct reason;
the background job leaves the verified result in core storage for the next
UI launch. The Android real-relay test prepares a second device's saved
claim, lets another device consume the link, and checks automatic handling
before and after coordinator restart. Other terminal fault cases and the
fixed-revision independent recheck remain due. The 64-page/16-MiB history
ceiling remains separate hardening.

## Advisory automatic-status recheck at ad42d78

Daybreak Blue at high thinking reviewed clean fixed commit
`ad42d781e132730b3222e3a0e45715b07f2e34d1` in a detached read-only
checkout through Yep Anywhere session
`01a0e364-2a07-7522-b3e9-ae9044b584f4`. The process reached verified
idle with an empty queue and no checkout edits. Result: **focused advisory
FAIL**; this is not the M0 exit review. The reviewer ran core, server, CLI,
Android build, and eight real-relay Android instrumentation tests. Remote CI
run `36328066358` passed all required jobs.

Two medium findings remain open. First, a signed `claimed` status does not
identify the claimant. A lost response to our own committed claim can make
the newly durable status row permanently stop a valid pending join. The
existing Android test checks a competing claim but not an in-call lost POST
response. Second, the read and `INSERT OR IGNORE` of a terminal status use
separate transactions, so concurrent foreground and scheduled callers can
display different signed reasons even though one row won storage. FS37 and
FS77 need regression cases before these guarantees close. The current build
is a development MVP and is not cleared for real Family data. The reviewer
also noted that the 64-page/16-MiB join history ceiling and full later-device
admission remain open.

The implementation priority now returns to missing MVP flows. These advisory
findings stay tracked for the M0 exit gate; the focused FAIL does not halt
independent feature work or call for a review after each small repair.

## Advisory MVP baseline preflight at 014f79b

Daybreak Blue at high thinking reviewed clean fixed commit
`014f79b6b8932c7df77529db1fbfb9dcaabea514` in detached checkout
`/tmp/babytrack-review-sy0COd` through Yep Anywhere session
`01a0e792-c8da-7b70-b4b8-20809591bc65`. The process reached verified idle
with an empty queue. Result: **directional advisory FAIL**, not the M0 exit
gate. The reviewer inspected current architecture, v1 protocol, scenarios,
Rust enrollment/store, Android coordinator and relay tests, and ran workspace,
native, browser, and Android build checks. It did not run an emulator or
physical-device flow.

The one immediate baseline join blocker was a saved recipient claim whose
exact retry committed at the relay but lost its HTTP response. Android then
persisted the signed invitation `claimed` reason and stopped automatic
progress forever, even though this device was the claimant. A signed v1
status identifies the link but not the claimant. The implementation now
keeps reason `2` inconclusive for a saved candidate, retains exact retry and
verified-control reconciliation, and lets a new link-only attempt report a
verified consumed link. Core refuses to persist reason `2` for a saved
attempt. The Android real-relay FS37 regression drops the accepted retry
response, restarts the coordinator, then completes challenge, proof, grant,
and child history. FS77 checks a link-only claimant sees the signed terminal
reason while a saved competing attempt stays unknown. This repair changes no
v1 wire bytes. The reviewer did not independently recheck it; the broader
M0 exit gate remains the next independent review.

The reviewer also noted the concurrent terminal-status insert race, the
Android 64-page/16-MiB join-history ceiling, and later-invitation lost
response coverage. Track those with the mixed-client fault matrix before
real Family data; do not serially re-review each feature slice. The current
test signal is an emulator with an honest local relay. It does not establish
physical-phone scheduling or malicious-relay availability.

## Completion condition

All slice gates pass in CI and on the designated local testbeds, the
cross-language byte suite agrees, mixed clients converge through the real
relay, and independent security review clears real-data use. Update this
file's boxes and index only with the changes that make them true.
