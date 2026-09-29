# M-1 protocol vectors

These are normative version-1 inputs and expected results. The shared Rust
core owns wire and data semantics; Swift, Kotlin, and wasm call it through
bindings. The M0 plan currently calls for versioned vectors in every
language. The table below records the *current* executable consumers, not a
claim that every published case is covered or that the M0 gate passed. A
named JSON file in a test is insufficient
evidence unless the test asserts the relevant case's expected bytes or state.
Hex is lowercase bytes, IDs are fixed 16-byte hex,
and each case starts from its stated state. Rejection means no state, cursor,
sequence, or outbox mutation unless the expected result says otherwise.

- [Canonical bytes and record projection](records-v1.json)
- [Deterministic cryptographic bytes](crypto-v1.json)
- [Complete genesis and signed encrypted batch bytes](full-wire-v1.json)
- [Challenge, admission, rotation, and keyring bytes](join-rotation-v1.json)
- [Contiguous signed join, batch, and removal chain](contiguous-chain-v1.json)
- [Exact object-free manager control bytes](no-object-controls-v1.json)
- [Encrypted minor-version and inert-batch negatives](negative-batch-v1.json)
- [Unsigned-byte array ordering](ordering-v1.json)
- [Object stage, control commit, and fetch bytes](api-v1.json)
- [Genesis reservation, stage, and promotion result](api-genesis-v1.json)
- [Signed invitation expiry boundary](invitation-boundary-v1.json)
- [Signed terminal invitation status](invitation-status-v1.json)
- [Sharing state machine and adversarial cases](sharing-v1.json)
- [Portable file and recovery](portable-file-v1.json)

## Execution and applicability matrix

| Vector | Owning executable consumer | Swift / Kotlin | wasm / browser | Remaining gap |
| --- | --- | --- | --- | --- |
| `records-v1` | `core/tests/cbor_vectors.rs` runs CB01–06; `crypto_vectors.rs` runs HASH01–02; `record_validity.rs` exercises published units | Shared core through local create, activity, backup/restore; no direct canonical-byte runner | Shared core through local journal and fixed child | MERGE, SCOPE, RECORD, TIME, and full UNIT case assertions need an explicit case inventory |
| `crypto-v1` | `core/tests/crypto_vectors.rs`, `hpke_vectors.rs`, `portable_file.rs`: SIG01, AEAD01, HPKE01 open and sender round trip; FILEBYTE01 protected bytes open and reject tampering | Fixed signed/encrypted batch crosses both bindings; wrong key and signer fail | Same fixed batch and rotation grant opening | HPKE01 fixed sender bytes and FILEBYTE01 derived-key/encode assertions remain |
| `full-wire-v1` | `core/tests/control_genesis.rs`, `batch_vectors.rs`: GENESIS01 and all BATCHBYTE01 constructed bytes | BATCHBYTE01 seals, opens, and projects | BATCHBYTE01 seals, opens, projects, and verifies its accepted receipt | Native bindings use a fixture projection rather than the full public authority chain |
| `negative-batch-v1` | `core/tests/batch_vectors.rs`, `projection_vectors.rs`, `operation_vectors.rs` cover selected cases | CROSSMINOR, INERT, PRECREATE, SETTHENCREATE, WRONGSCOPE, PREFS plus wrong key, Family, relay, signer, signature | Same selected cases; Chromium checks durable rollback | UNOPENABLE and exact per-case assertion inventory remain |
| `join-rotation-v1` | Published fixed objects; production join/rotation has `core/tests/control_chain.rs` and relay flow tests using the contiguous chain | Native shared store and Android emulator exercise dynamic join/rotation, not these fixed hex fields | wasm/Chromium checks the contiguous chain through rotation | CHALLENGE, ADMISSION, ROTATION exact-case byte assertions remain |
| `contiguous-chain-v1` | `core/tests/control_chain.rs`, `shared_history.rs`, `shared_ready.rs`; `server` and CLI relay cases replay signed controls, batch, and removal | Android two-emulator dynamic flows, not this exact chain | wasm/Chromium replays through first rotation; browser real relay hydrates objects and writes | Full-chain case inventory and post-removal browser replay remain |
| `no-object-controls-v1` | `core/tests/control_chain.rs` checks all three published controls | Uses shared Rust reducers via native store | Public authority replay, no direct fixture consumer | Direct wasm and relay vector assertions remain |
| `ordering-v1` | No direct ORDER01 assertion | Shared Rust ordering only | Shared Rust ordering only | Exact case assertion missing |
| `api-v1`, `api-genesis-v1` | `server` HTTP/store tests and `core/src/sync_wire.rs` check signed paths, stages, and receipts | Not a client byte format except signed GETs | wasm/browser signed GET and staged genesis response | Per-route negative inventory remains |
| `invitation-boundary-v1` | Boundary behavior is covered in relay tests with injected clocks; no direct fixture consumer | Not a client clock authority | Browser consumes signed result, not relay clock | INVEXP01–02 exact-case assertion missing |
| `invitation-status-v1` | `core/src/bootstrap.rs` and relay tests assert signed status | Both bindings verify response and reject tampering | Browser verifies through shared Rust | No named gap for the signed response bytes |
| `portable-file-v1` | `core/tests/portable_file.rs` runs FILEBYTE08 and protected/readable round trips | Both bindings create, restore, and reject wrong-password files | Not yet a browser export/import adapter | FILE01–07 case inventory and browser file flow remain |
| `sharing-v1` | Symbolic state-machine expectations, no literal wire bytes | Exercised by selected native and emulator scenarios | Exercised by selected browser scenarios | Map all symbolic cases to scenario tests; not a four-runtime byte suite |

The cross-language event gate currently exercises one exact encrypted child
event and negative authentication through Rust, Swift, Kotlin, and wasm.
Only the Rust core owns CBOR and cryptographic implementation. The remaining
rows above keep the broader M0 byte gate open until explicit assertions and
scenario links are added; sharing symbolic cases are not byte fixtures.

The [scenario catalog](../../docs/scenarios/README.md) owns user-visible
observations; a vector's `scenario` field connects the lower-level assertion
to one or more of those cases. The contiguous chain supplies complete
genesis, issue, claim, challenge, proof, admission, repair, batch, and removal
transcripts. The object-free cancellation, role-change, and pending-removal
bytes are checked by the Rust control-chain test; other platform runners
have not consumed them yet. `INVSTAT01` runs through the shared Rust core
and the Kotlin and Swift production bindings, including signature tampering.
The wasm and Chromium harnesses now consume the contiguous chain through
its first rotation, with the signed receipt, grant, keyring, membership, and
reload checks. This is a selected chain case, not full vector coverage.
Negative state-machine cases remain in the smaller JSON files.
Cryptographic test keys
must never be production keys.
