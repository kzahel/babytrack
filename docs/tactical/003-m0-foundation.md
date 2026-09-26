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

Gate: no crash loses a committed local operation or publishes a partial
copy/restore; no Family handle reaches another Family's rows, keys, or file.

### 3. Relay authority and early security review

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
- [ ] Run the early M0 implemented-protocol review from the
  [MVP plan](../mvp-plan.md#security-review-gates) using the
  [review runbook](../security-review-runbook.md). Review authorization,
  invitation, encryption, and rotation at a fixed revision. Fix access and
  retention blockers before expanding mixed-client sync.

Gate: the relay and clients enforce the reviewed authority transitions;
deterministic authority and key-handoff tests pass; the independent review
has no unresolved access or retention blocker.

### 4. Mixed-client sync and recovery adversarial pass

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
browser outbox, and private copy remain open. Native SQLite now also reserves a random
batch ID and nonce and persists exact signed envelope bytes in one transaction.
An uncertain retry returns those bytes after reopen even if the caller's
current head/key has changed. The crate-private staging path awaits a
core-owned authority session. The initial `FamilySession` now stages through
that path only with its verified manager identity, head, epoch key, and
signing key. A matching relay-signed acceptance moves the exact envelope and
receipt to replayable SQLite history while clearing the outbox in one
transaction; a bad receipt leaves it pending through restart. Later control
epochs, browser outbox, and rebatching remain open.
The decoder now enforces UUIDv4 Family/device/batch identities, UUIDv7
operation/child/activity identities, and a positive batch epoch.
The Rust core now verifies the fixed `GENESIS01` committed control object:
relay pin and receipt signature, manager signature, transition core and
state hashes, promotion manifest binding, receipt context, and head hash.
Only this verified genesis can issue an epoch-key token after checking its
committed key commitment. Subsequent control transitions and persistence of
the pinned head remain open.
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
hashes, signatures, and cursor. Their HPKE/verifier objects and proof secret
still need validation before a holder signs admission. Public admission
now atomically replaces a
proved pending device with its active row at the invitation's fixed role
and commitment; repair names that admission and leaves authority unchanged.
The contiguous byte vector passes through both transitions. Validating the
HPKE grant and encrypted membership objects, producing these transitions,
and encrypted membership readiness remain open. Public replay now also
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
Membership ciphertext comparison and repair/rotation grants remain open.
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
keys for encrypted batches and the reusable local-only journal adapter;
complete browser sync storage and cross-language vector coverage remain open.

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

## Completion condition

All slice gates pass in CI and on the designated local testbeds, the
cross-language byte suite agrees, mixed clients converge through the real
relay, and independent security review clears real-data use. Update this
file's boxes and index only with the changes that make them true.
