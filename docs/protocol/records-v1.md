# Records and operations v1

Status: M-1 decided contract, September 2026; Rust canonical CBOR and
structural operation decoding have begun in M0. Complete field validity,
projection, and cross-language execution remain. This owns
version 1 plaintext operation bytes and deterministic projection. The
[event model](../topics/event-model.md) owns which values the product records;
[sharing v1](sharing-v1.md) owns encryption and authorization. All limits are
checked before allocation and apply to decrypted bytes too.

## Canonical values

All signed or encrypted protocol structures use RFC 8949 core deterministic
CBOR. Only definite-length unsigned/signed integers, byte strings, UTF-8 text,
arrays, maps, booleans, and null are legal. Reject floats, tags, indefinite
lengths, duplicate keys, non-shortest integer/length encodings, invalid UTF-8,
and unsorted map keys. Protocol map keys are unsigned integers. An unknown
value may use any of these allowed types and is retained as its exact
canonical CBOR bytes. A decoder must reject noncanonical input rather than
parse then silently normalize it. Wire integer ranges are checked after CBOR
decoding; a `u64` does not fit a negative integer.

UUID values are 16 network-order bytes; activity, child, and operation IDs
are generated as RFC 9562 UUIDv7. Family, device, transition, invitation,
batch, and grant IDs are independently random 16-byte UUIDv4 values. IDs are
never inferred from display names, account IDs, HLC values, or relay cursors.
Hashes are 32-byte SHA-256 outputs. `u16`, `u32`, and `u64` below mean checked
nonnegative integers in those ranges, not fixed-width CBOR encodings.

Version 1 rejects a structure with an unknown required top-level version or
unknown operation kind. Unknown record types and field IDs are permitted.
Known maps may contain additional positive integer keys: preserve them in
storage and on byte-preserving re-encryption, but never give them authority
semantics. Control-plane structures in sharing v1 do **not** permit unknown
keys in version 1 because a client cannot safely ignore new authority rules.

## Record scopes and field registry

An operation acts on one record in one Family. Scope `1` is family metadata,
requires `record_type="family"`, and has `record_id = family_id`; scope `2`
is child metadata, requires `record_type="child"`, and uses the child ID as
record ID; scope `3` is child activity, forbids those two reserved types,
and requires an immutable child ID whose valid child create precedes the
activity in verified log order or earlier in the same batch. Record type and
scope cannot change after create. Deleting a child metadata
record does not delete activity records. Moving an activity to another child
requires an explicit new activity and deletion of the old one.

Field keys are unsigned integers in a map. `null` means the field is present
with no value; absence means untouched. Shared activity keys:

| Key | Meaning | CBOR value |
|---|---|---|
| 1 | start | `[utc_ms, offset_minutes]` |
| 2 | end | instant or null |
| 3 | group | 16-byte UUID or null |
| 4 | note | UTF-8 text or null |

The instant is UTC milliseconds since Unix epoch as a signed 64-bit integer,
paired with the recorded offset in minutes (`-840..840`). Its offset does not
alter the instant. Activities require field 1 on creation. A time interval
may cross a daylight-saving or zone boundary; each endpoint stores its own
offset. Durations subtract UTC instants. Calendar reports use midnight in
the viewing device's zone and split spans at that zone's day boundaries.
Backdated activity start never changes its operation HLC.

Type-specific field IDs are scoped to the immutable `record_type` string:

| Type | Keys from 100 | Version 1 value constraints |
|---|---|---|
| `feed.breast` | 100 segments | Array of `[side, start, end-or-null]`; side 1 left, 2 right; one atomic field |
| `feed.bottle` | 100 amount, 101 content | Measure; content 1 breast milk, 2 formula, 3 mixed, 4 other |
| `feed.solids` | 100 foods, 101 amount | Array of text; text amount |
| `sleep` | 100 place | 1 crib, 2 pram, 3 contact, 4 car, 5 other, or null |
| `pump` | 100 left, 101 right, 102 total | Measure or null; reject total together with left/right |
| `diaper` | 100 kind | 1 wet, 2 dirty, 3 both, 4 dry |
| `growth` | 100 weight, 101 length, 102 head | Measure or null |
| `medication` | 100 name, 101 dose | Text; `[amount-text, unit-text]` |
| `temperature` | 100 value, 101 method | Measure; text or null |
| `note` | no additional keys | Shared note field 4 is required |

An unknown `record_type` and its entire field map remain opaque and
round-trip unchanged. A known type with an unknown field retains the field's
canonical value bytes even when another known field is edited or the batch
is re-encrypted. New code may render unknown activity as an unsupported entry
without discarding it. Type-specific schema revisions use new field IDs,
never reinterpret a published key.

Family metadata field `1` is display unit preferences, a CBOR map from
`1=volume, 2=mass, 3=length, 4=temperature` to one published unit code of
that dimension. Each key occurs at most once; absent means use device/locale
default. The accepted values are volume `1..3`, mass `10..13`, length
`20..22`, temperature `30..31`. A per-device override is local setting,
not a shared operation.
Child metadata fields: `1` name text, `2` birth date as signed days since Unix
epoch, `3` WHO sex code (`1` female, `2` male, `3` unspecified; unspecified
suppresses percentile). Additional fields are preserved opaquely. Names and
all field values occur only inside encrypted batches or user-selected files.

A measured value is CBOR map `{1: base_integer, 2: entered_decimal_text,
3: entered_unit_code}`. The entered decimal is ASCII `-?(0|[1-9][0-9]*)(\.[0-9]+)?`
with no exponent or redundant leading zero; the entered unit code fixes the
dimension. Base units are mL, g, mm, and hundredths Celsius. Conversions use
exact decimal rational constants and round to nearest base integer, with
half ties away from zero. Millilitre factors: mL `1`, US fl oz
`29.5735295625`, UK fl oz `28.4130625`. Gram factors: g `1`, kg `1000`, lb
`453.59237`, oz mass `28.349523125`. Millimetre factors: mm `1`, cm `10`,
inch `25.4`. Fahrenheit to Celsius-hundredths is `(F-32)*500/9`; Celsius is
`C*100`. A decoder recomputes base from entered data and rejects mismatch.
Display preserves the entered decimal and unit when available.

Unit codes are `1=mL, 2=US fl oz, 3=UK fl oz, 10=g, 11=kg, 12=lb,
13=oz mass, 20=mm, 21=cm, 22=inch, 30=Celsius, 31=Fahrenheit`. A known
measured field rejects a code from another dimension. Growth weight uses
mass, length/head use length, bottle and pump use volume, and temperature
uses temperature. An unknown unit code in a known measured field is
preserved as an opaque value for that field. An older client may edit other
fields but cannot convert or edit that measured field until it understands
the code.

## Operation format

One operation is canonical CBOR map with these integer keys:

| Key | Field | Type |
|---|---|---|
| 1 | format version | `1` |
| 2 | Family ID | bytes16 |
| 3 | operation ID | UUIDv7 bytes16 |
| 4 | record ID | bytes16 |
| 5 | scope | `1..3` |
| 6 | kind | `1=create, 2=set, 3=delete, 4=restore` |
| 7 | author device ID | bytes16 |
| 8 | HLC | `[wall_ms_i64, counter_u32, device_id_bytes16]` |
| 9 | record type | UTF-8 string on create, otherwise absent |
| 10 | child ID | bytes16 on activity create, otherwise absent |
| 11 | changed fields | map on create/set, otherwise absent |

The HLC device ID equals key 7. The counter is limited to `0..1000000`.
HLC records the author's causal estimate; it is not the conflict winner or
authorization clock. `create` sets immutable scope/type/child and
initial fields. `set` has at least one changed field and cannot edit those
immutable values. `delete` sets the tombstone true; `restore` sets it false.
These kinds have no field map. The Family ID and author must match the signed
batch envelope; an operation with a different value invalidates the entire
batch. Max encoded operation size is 64 KiB, max text field 16 KiB, max field
count 128, max nesting depth 16. An operation ID is unique per Family and
stable across batch retries or re-encryption. Repeating the same ID with
different bytes is a conflict, never a second edit.

Exactly one distinct `create` is permitted for each `(family_id, record_id)`.
A second create, even with a different operation ID or matching content, is
a record conflict; the entire containing batch is inert. A `set`,
`delete`, or `restore` requires a valid create earlier in verified log order
or earlier in the same batch; otherwise the whole batch is immediately
inert. No later create can change that verdict. Operations on the wrong
scope/type or from a
nonmember author at their committed cursor are invalid.

The complete operation CBOR bytes are the payload unit in a batch. A client
that does not understand a field retains its raw value and never rewrites an
existing operation. A new edit is a new operation. Re-encrypting a pending
operation after an epoch change uses its identical operation bytes, including
ID and HLC, in a new batch.

## Merge and clock rules

For each field, the shared projection selects the value at the greatest
verified relay cursor and, within a batch, greatest zero-based operation
index. Locally created history before first sharing has a durable local
append index; promotion declares those operations in that order before
cursor 1. A private copy or file restore creates a new local order. The
tombstone is a separate field with the same ordering. When tombstoned,
record fields remain retained but hidden from ordinary timelines; later
field edits do not themselves restore it. Restore is an explicit operation.
All valid operations, including losing field values, remain in the log and
can be inspected. Restoring a displaced value makes a new `set` operation;
it does not delete history. A relay order is the winner for the shared
Family; a malicious relay can fork this order as already accepted.

Each device persists its last HLC per Family. Before a local operation, let
`now` be local wall time, `local=(lw,lc)`, and the greatest received stamp
`remote=(rw,rc)`. Set `w=max(now,lw,rw)`. If `w=lw=rw`, set
`c=max(lc,rc)+1`; if only `w=lw`, set `c=lc+1`; if only `w=rw`, set
`c=rc+1`; otherwise set `c=0`. Persist it with the operation in one local
transaction. If increment would exceed `1000000`, advance the logical wall
by one millisecond and reset the counter to zero. At `i64::MAX`, preserve
the edit with the last valid local stamp and show a clock anomaly; this
does not block writing because HLC does not choose the winner. A received
stamp more than 24 hours after its verified relay commit time is not fed
into the local HLC generator and is flagged. Offline concurrent writes
converge by eventual verified log position. HLC is not authorization time
and does not date the activity.

A signed and authorized batch whose decrypted payload violates the
major-version-1 shared-validity rules below is an inert data entry: every
client records its cursor, batch hash,
and validation error, applies none of its operations, and continues replaying
later entries including control transitions. It is visible as a sync
integrity issue. Same operation ID with different bytes is likewise inert
at the later entry; the first valid occurrence wins deduplication.

Native SQLite and browser IndexedDB must atomically append accepted
operations, update the projection, persist the HLC/outbox state, and dedupe
operation IDs. A crash may replay an operation but must not apply it twice.
No record crosses Family storage handles or keys. The exact local schema is
an implementation detail as long as these transaction and isolation
invariants hold.

## Version skew

Batch protocol major `1` requires these semantics. Minor additions use new
field IDs and record types. **Shared validity is frozen for major version
1:** whole-batch inertness may depend only on canonical CBOR, operation
shape/limits, Family and author binding, duplicate IDs/creates, immutable
scope/type/child rules, and constraints on fields published in version 1.
A future minor field or record type is always a canonical opaque value for
projection and backup, even to a newer client that recognizes its semantic
schema. If such a value is malformed for display or editing, that client
marks only that field/type unrenderable and reports it; it does not make the
batch inert or discard another field. New validity constraints require a
new major version. Thus two v1 clients project the same raw winners from
one verified log despite different minor versions. A client that sees a
newer minor retains unknown bytes and may edit known fields. On a newer
major version it may read what it can but must stop writing to that Family
and request an update; it cannot downgrade and re-encrypt unsupported
content.

Reference standards: [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949),
[RFC 9562](https://www.rfc-editor.org/rfc/rfc9562).
