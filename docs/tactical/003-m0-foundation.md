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
them. The caller supplies the fixture's nonce, batch ID, and sequence;
production allocation and retry ownership belong to the durable outbox.
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
and two-Family isolation. Materialized projection, accepted shared entries,
outbox, production browser IndexedDB, and private copy remain open.
The decoder now enforces UUIDv4 Family/device/batch identities, UUIDv7
operation/child/activity identities, and a positive batch epoch.
`bash scripts/check_browser_smoke.sh` launches Playwright's isolated Chromium
shell, loads the wasm binding, and checks IndexedDB reload, an aborted
multi-store transaction, and Family-scoped keys and rows. It uses fixture
keys and a small test adapter; the production browser store and complete
cross-language vector suite remain open.

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

## Completion condition

All slice gates pass in CI and on the designated local testbeds, the
cross-language byte suite agrees, mixed clients converge through the real
relay, and independent security review clears real-data use. Update this
file's boxes and index only with the changes that make them true.
