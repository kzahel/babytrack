# Protocol and behavior vectors

M-1 records agreed behavior here before M0 implementation. The symbolic
[family lifecycle cases](vectors/family-lifecycle.json) state product
invariants that the Rust, Swift, Kotlin, and TypeScript suites will exercise
once the shared core and bindings exist. They are not a wire format or crypto
known-answer vectors. M-1 must add versioned encoding, crypto, time, and
adversarial sync vectors before code depends on those details.

The symbolic [record projection cases](vectors/record-projection.json) state
that the append-only log and rebuildable current-record view agree, including
edits, tombstones, and backdated activity times.

The [sync topic](../docs/topics/sync-and-encryption.md) owns membership and
keys; the [event-model topic](../docs/topics/event-model.md) owns records.
