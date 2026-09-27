# M-1 protocol vectors

These are normative version-1 inputs and expected results. M0 binds each
case to Rust, Swift, Kotlin, wasm, and relay runners. The Rust core currently
runs CB01-CB06 in `records-v1.json` and selected operation-byte assertions
from `full-wire-v1.json` and `negative-batch-v1.json`. Hash, Ed25519, and
XChaCha cases in `crypto-v1.json` also run; `HPKE01` is checked for fixed
decryption and sender round trip, with fixed seal bytes still open.
`BATCHBYTE01` in `full-wire-v1.json` matches all constructed bytes and opens.
No other passing result is implied.
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

The [scenario catalog](../../docs/scenarios/README.md) owns user-visible
observations; a vector's `scenario` field connects the lower-level assertion
to one or more of those cases. The contiguous chain supplies complete
genesis, issue, claim, challenge, proof, admission, repair, batch, and removal
transcripts. The object-free cancellation, role-change, and pending-removal
bytes are checked by the Rust control-chain test; other platform runners
have not consumed them yet. Negative state-machine cases remain in the
smaller JSON files.
Cryptographic test keys
must never be production keys.
