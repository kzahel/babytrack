# Portable Family file v1

Status: M-1 decided contract, September 2026. This is the exact backup and
independent-restore contract. A file contains a snapshot of locally held
record state, not a credential for the source Family. CSV remains an analysis
export, not this backup format.

## Readable form

The file is UTF-8 JSON Lines with one compact JSON object and one LF per
line, no BOM, blank lines, comments, NaN, duplicate object keys, or trailing
bytes. The first line is the header, subsequent lines are child and record
rows sorted by `(kind, source_id)` in unsigned byte order, and the last line
is a trailer. JSON integers are decimal, with no fraction or exponent.
Opaque CBOR field values are lowercase unpadded base64url strings. A writer
uses sorted object keys, UTF-8 without ASCII escaping except required JSON
escapes, and no insignificant whitespace. A reader accepts equivalent valid
JSON but checks semantic constraints before allocating or writing data.

Header keys are exactly `kind:"babytrack-backup"`, `version:1`,
`source_family_id` (lowercase UUID), `snapshot_utc_ms` (signed i64),
`source_cursor` (u64 or null for local-only), `known_gap` (boolean),
`child_count` (u32), `record_count` (u32), and `state_sha256` (64 lowercase
hex digits). `state_sha256` is SHA-256 of the exact concatenated UTF-8 child
and record lines, including LF. Each child row has `kind:"child"`,
`record_type:"child"`, `source_id`, `deleted` boolean, and `fields`: an object from decimal field
ID to canonical CBOR value encoded as base64url. Each record row has
`kind:"record"`, `source_id`, `source_child_id` or null, `scope` (`1` or
`3`), `record_type`, `deleted`, and `fields`. One family metadata row uses
scope `1`, `record_type:"family"`, and its source ID is the header's Family
ID; activities use scope `3`, a child ID, and a nonreserved activity type.
Children are scope `2` rows with `record_type:"child"`. The trailer is exactly
`{"kind":"end","rows":N}` with N equal to child_count + record_count.
Unknown field IDs and values remain byte-identical canonical CBOR. Future
minor-version fields may refer to child/record IDs, but may not embed the
source Family ID or another authority identity; an extension needing that
must use a new major version with an explicit relocation rule. A writer
exports the current winner and tombstone for each record, including deleted
records and locally pending edits reflected in the projection. Full source
edit history is deliberately omitted; live sync and a private log copy retain
it. The header marks a known sync gap; a file can never claim to contain
unreceived remote history. The UI displays the snapshot time and gap before
and after export.

No row contains a signing/agreement private key, epoch key, relay credential,
membership role, invitation, grant, source outbox, or original authority.
Source child and record IDs are retained under a fresh local Family ID so
opaque references between records remain valid. The composite
`(family_id, record_id)` identity prevents collision with the source Family.
The one Family metadata record ID becomes the new Family ID. Restore creates
fresh device keys, epoch key, operation IDs, and current-state create
operations. It does not resume the source cursor or attach to the source
relay. Restore validates counts,
hash, all references, field types, IDs, size limits, and target schema before
one local transaction publishes the new Family. Repeated restore is a new
independent Family only after a new explicit user action. Error or crash
before commit leaves the destination absent and the input file untouched.

Max file size is 2 GiB, max row size 1 MiB, max rows 1 million, max nesting
depth 16, max text value 16 KiB, and each decoded field value is at most
64 KiB. Unknown future major versions and unknown required keys are rejected
without partial restore. Version 1 writers may add unknown positive field
IDs but not new top-level keys. A reader rejects duplicate source IDs or
missing child references. The `state_sha256` detects accidental damage but
does not authenticate a readable file against deliberate modification.

## Optional protected form

The protected file starts with ASCII `BTBK1` followed by one canonical CBOR
header and encrypted payload. The fixed header is map `{1:1,
2:argon2id_salt16, 3:65536, 4:3, 5:4, 6:xchacha_nonce24,
7:plaintext_length_u64}`. Fields 3-5 are Argon2id memory in KiB, passes,
and lanes. The password is UTF-8 NFC text; Argon2id v1.3 derives a 32-byte
key. The AEAD is XChaCha20-Poly1305 with AAD equal to `BTBK1` plus the
exact header bytes and plaintext equal to the full readable file. The
encrypted payload includes its 16-byte tag and has exactly
`plaintext_length + 16` bytes. Reject oversized lengths and unsupported
parameters before KDF/allocation. A wrong password, tampering, truncation,
or invalid plaintext fails without partial restore or a distinguishable
password-vs-corruption message. Every export uses a random new salt and
nonce. The selected password is never stored as a Family credential.

The Argon2id parameters follow the memory-constrained recommendation in
[RFC 9106](https://www.rfc-editor.org/rfc/rfc9106). Protected file export
must check device memory and report failure if this profile cannot run; it
must not silently weaken the KDF. Readable export remains available by
explicit user choice.
