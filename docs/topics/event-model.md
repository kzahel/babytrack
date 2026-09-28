# Event model

Status: M-1 event decisions and v1 record/portable-file contracts agreed,
September 2026; M0 execution remains.
Owns what gets logged: entities, event types and fields, timers, units, time
and day boundaries, multiple children, import mapping, and export. How
events are stored, merged, and synced is owned by
[sync-and-encryption.md](sync-and-encryption.md).
The [Family sharing contract](family-sharing-and-trust.md) owns user-visible
retention of pending edits after removal and the open displaced-edit UX
(D6). Its scenario IDs link those promises to later implementation tests.

## Entities

- **Family.** The user-facing name for an independent tracking and sharing
  space; it need not describe a biological or legal household. It holds
  settings such as display units. One app can hold multiple Families.
- **Child.** Name, birth date, and sex (WHO growth charts are sex-specific).
  A family has any number of children.
- **Holder.** An enrolled Family-specific device credential, defined in the
  sync topic. A portable file is data, not a key holder or access credential.
  Each event records which device wrote it.
- **Activity event.** A child-scoped item in the timeline. Child and family
  metadata also use the shared operation and sync machinery, but their
  envelope and scope are fixed in [records v1](../protocol/records-v1.md).

## Event envelope

Every child-scoped activity event has:

- `id`: UUIDv7, created on the device that logs it;
- `child`: the child it belongs to;
- `type`: one of the types below, or an unknown future type;
- `start` and optional `end`: UTC instants in milliseconds, each with the
  UTC offset in minutes where it was logged;
- `group`: optional id linking events logged together, such as one feed
  given to twins;
- `note`: optional free text;
- type-specific fields.

Each field merges independently (last writer wins), so fields are kept small
and independent. The operation log retains displaced edits; record history
lets a caregiver inspect one and restore it by making a new edit. A field
whose parts must change together, such as a list of feeding segments, is a
single field in the MVP. Concurrent edits to that list therefore choose one
complete list, with the other inspectable in history.

## Event types for the MVP

| Type | Fields |
|---|---|
| `feed.breast` | `segments`: list of side (left or right), start, end |
| `feed.bottle` | `amount`, `content` (breast milk, formula, mixed, other) |
| `feed.solids` | `foods` (list of text), `amount` (text) |
| `sleep` | start and end; optional `place` (crib, pram, contact, car, other) |
| `pump` | start and end; `left` and `right` amounts, or `total` |
| `diaper` | `kind` (wet, dirty, both, dry) |
| `growth` | any of `weight`, `length`, `head` |
| `medication` | `name`, `dose` (amount and unit text) |
| `temperature` | `value`, optional `method` |
| `note` | text only |

Android records weight, length, and head circumference in any nonempty
combination in one growth event. Entry and correction accept the published
g, kg, lb, and oz mass units, and mm, cm, and inch length units; the form
defaults to kg and cm. Rust uses the published exact factors and rounding,
stores the base grams and millimetres, and preserves each entered decimal
and unit for the timeline. The analysis CSV exports base units. Empty or
out-of-range measurements are rejected, and a blank edit leaves the saved
field intact. Local restart, file restore, and encrypted two-device sync
preserve the entered values and units.
Temperature entry and correction now accept Celsius or Fahrenheit. The core
parses the entered decimal, applies the published exact conversion to
hundredths Celsius, and retains the entered text and unit for display. Local
restart/file restore and cross-device encrypted sync preserve these
measurements. Medication entry records a name and entered dose amount/unit
as text without a dosing recommendation; local file restore and encrypted
cross-device sync preserve the fields. Bottle logging and correction now
accept mL, US fl oz, and UK fl oz. The Rust core converts entered decimals
with the v1 exact factors, while the Android timeline retains and displays
the entered text and unit. Other entered units and advanced editing remain
M1 work. Solids entry stores one trimmed food per line
as the published atomic list and preserves optional entered amount text;
local file restore and encrypted cross-device sync preserve both.
The breast-feed path records one to eight completed timed left/right
segments as one atomic field. Android can add alternating side durations;
the core checks contiguous intervals, side values, offsets, and a total
duration of at most four hours. Each segment keeps its start and end UTC
offsets, including across a daylight-saving transition. Local files and
encrypted shared sync preserve the segment sequence.
Android can optionally mark a running or completed sleep as crib, pram,
contact, car, or other. The shared core stores the published place code on
the sleep event. A caregiver can correct or clear the place on that same
event, including while its timer runs; stopping or correcting its duration
retains the place field.
The timeline and analysis CSV show a recorded place, while older sleep
entries without it remain valid.
Correcting a completed breast feed replaces its segment list and derived end
instant in one set operation while retaining its activity ID, original start,
and unrelated or unknown fields. The core applies the same interval, side,
offset, and four-hour checks as creation. Android offers whole-minute side
and duration correction for entries whose existing segments are whole
minutes; other clients' finer-grained entries remain readable and deletable.
The Android timeline can delete one activity. The shared core checks its
Family and child target and records a tombstone operation; the entry leaves
the current timeline locally and on devices that later sync. Deletion does
not erase an already received copy or rewrite the historical operation log.
The timeline can also edit a note's text. The shared core checks the exact
Family, child, live note type, and text length, then writes a field-set
operation on the same activity ID. Its original time and unknown fields
remain intact; the corrected value survives offline replay and file restore.
Bottle amount correction similarly replaces only the whole-millilitre
measurement field on the same activity. The core validates its live
Family/child target and amount, retaining the original time, content code,
and unknown fields for replay and sync.
Android now offers breast milk, formula, mixed, or other when logging a bottle
and shows the saved content in its timeline. Its bottle edit can replace both
the amount and content on that same activity in one field-set operation;
neither change moves the original event time. The older amount-only core edit
continues to preserve content.
Solids correction replaces the complete foods list and entered amount on the
same activity. The core validates the live Family and child, applies the same
food and length limits as creation, and retains the original time and unknown
fields. Both values survive file restore and encrypted shared sync.
Pumping records a completed interval with either separate left/right
millilitres or one total, never both forms in one entry. It does not create a
freezer inventory balance.
Pumping amount correction uses a field-set operation on the same activity.
It clears the unselected amount form, preserving the interval, child target,
and unknown fields.
Medication name and entered dose corrections likewise update the same live
activity, preserving its original time and other fields. They remain entered
facts; the app does not interpret or recommend doses.
Child birth day and growth-chart sex may be corrected by field-set operations
on the same child ID. A missing input leaves the saved field intact; the
existing protocol does not clear a previously recorded birth day. Activities
remain attached to that child.
Growth corrections may set weight, length, or head circumference on the same
activity ID. A blank value in the Android edit leaves that measurement as it
was; deleting the entry removes all its measurements from the timeline.

Growth percentiles are computed from WHO tables when shown, never stored.
No WHO reference table is bundled yet. WHO's published dataset terms permit
reuse for public-health purposes with attribution but restrict use alongside
promotion of a commercial product; its general copyright page also calls for
permission in a commercial context. Those terms are not an MIT grant for the
tables. Resolve the specific table's reuse rights or obtain permission before
adding reference data to this repository or a distributed build. Growth
logging and correction remain usable without the percentile display.
Wake-window hints are computed from sleep events on the device.
Freezer milk inventory is outside the MVP; pumping records do not imply a
stock ledger or `stash.add`/`stash.use` operations.
Diaper colour and consistency are outside the MVP.

## Timers

A running timer is an event with a `start` and no `end`. Stopping it sets
`end`. Because timers are ordinary events they sync like any other, so a
feed started on one parent's phone shows as running on the other's watch.
Breastfeeding side switches and pauses are segments inside the one event.
Two devices both running a timer for the same activity produce two events;
the UI offers to merge them rather than guessing.

## Units

- Stored as integers in base units: millilitres, grams, millimetres, and
  hundredths of a degree Celsius. Integers keep merge and test vectors exact.
- Every measured field also records what the user entered, as a value and a
  unit, so 4 oz is shown back as 4 oz rather than 3.99 oz after conversion.
- Display units are a family setting with a per-device override. US and UK
  fluid ounces differ (29.57 ml against 28.41 ml) and both are supported.

## Time and days

- Instants are UTC. The offset recorded with each instant is where the
  caregiver was at the time.
- Durations, intervals since the last feed, and wake windows use instants
  only, so they are unaffected by time zones or daylight saving.
- A caregiver may log or import an activity whose `start` is in the past.
  The activity time controls where it appears in timelines and reports; the
  operation's HLC records the author's causal estimate; verified log order
  controls shared merge order.
  Backfilling does not require rewriting old operations.
  Android's logging screen can select an earlier local date and time for the
  next completed entry. The core receives that event time with the offset at
  the selected instant, while the operation is authored when it is saved.
  Starting a running sleep timer always uses the current time. The selected
  earlier time clears after a successful completed entry or a child/Family
  switch; a failed save leaves it available for retry.
- Lists and timelines show each event at the local time it was logged, with
  a marker when that offset differs from the viewing device's current one.
- A day is a calendar day in the viewing device's current time zone. Events
  are listed on the day they start. Daily totals, such as sleep, split a
  duration that crosses midnight between the two days.
  Android's timeline groups current entries by that local start day; its
  category filters do not change the saved records or their order.
- The MVP uses midnight as day start and has no configurable day boundary.
  Family reports use the viewer's zone too, so caregivers in different zones
  may see different daily totals for the same instants.
- Travel and daylight saving cases are written into `tests/vectors/` as test vectors.

## Multiple children

Every activity event belongs to exactly one child in exactly one Family.
Child identity and display name are scoped to that Family. Two Families may
each represent the same real child but keep separate histories unless a user
explicitly imports records into one of them. Logging for twins at once
creates one event per child sharing a `group` id, so each child's history
stays complete and either event can be edited alone.
Android clears unsaved logging drafts and pending edit dialogs when the
selected child or Family changes, so input prepared for one target does not
carry into another. An unsuccessful completed-entry save keeps its draft for
retry; a successful save clears only the values it submitted.

## Unknown types and fields

Unknown types are shown as an entry that needs a newer app version and are
kept unchanged. Unknown fields on a known type are kept and written back
unchanged on edit. This is the forward compatibility requirement from the
plan applied to events.

## Import and export

- **Export and file backup.** Readable CSV supports spreadsheets and analysis.
  A documented full structured export also serves as a restorable Family
  backup, with optional password protection. The working format is versioned
  JSON Lines with base units. It includes family settings, children, current
  activity state, tombstones, pending local state at the saved point, and
  unknown fields. It does not promise the source operation/edit history;
  displaced values remain inspectable in a live Family or private log copy,
  not after state-only file restore. CSV is not a full-fidelity backup.
  The first analysis CSV writes one row per current activity with Family,
  child, time, type, and known entered fields, including bottle content.
  Child birth day and growth-chart sex travel with each row. Deleted
  activities, operation history, unknown
  fields, and access credentials are absent; use a portable file for recovery.
  JSON arrays inside cells preserve solids foods and breast segments. Text
  beginning like a spreadsheet formula is prefixed with an apostrophe, so
  analysis CSV is not a byte-exact text interchange format.
- **File restore.** Restore the saved record state into a new independent
  local-only Family with fresh Family identity and keys while retaining
  child/activity record IDs inside that new Family scope, offline and without the
  original group's permission. Include saved pending local work in the
  snapshot. Existing Families remain unchanged. Files contain data, not
  original-Family device credentials or authority to rejoin. The user can
  share the new Family and invite caregivers again. Protected and readable
  forms must restore equivalent saved data; the protected form also requires
  its protection credential. See product decision D3.
- **Import** maps Nara, Huckleberry, and Nighp exports onto these types.
  When a source has stable record IDs, derive imported UUIDv5 IDs from the
  source app, source account/export namespace, and that record ID. Repeated
  or overlapping exports update the same destination record by field diff;
  they do not create duplicates. If a source supplies no stable ID, derive
  identity from normalized content and show a preview warning that edited
  rows may import as new records; never use row number as identity. Exact
  mappings await real anonymized samples.
- **No automatic family merge in the MVP.** Competing live families, child
  identities, memberships, and operation histories are not merged. Members
  may keep both Families in one app and switch between them without merging.
  An activity created while one Family is active belongs only to that Family.
  Members can backfill entries manually, and the planned competitor CSV import
  can create entries in the chosen family. A later client-side import tool may
  copy selected records from a local family into another as new operations,
  mapped to a chosen child; this is data import, not sync-history merging.
  A later CLI may expose this through the shared core, with explicit child
  mapping, duplicate/conflict handling, and a preview. It remains outside
  the MVP and must not reimplement event or merge semantics in scripts.

## Validation

- Vectors for unit conversion and display rounding.
- Vectors for day boundaries across midnight, daylight saving changes, and
  travel between zones.
- A backdated activity appears at its activity time while its operation
  retains its author HLC metadata and merges by verified log order.
- Round-trip tests: every type through encode, merge, export, and import.
- Full backup restores saved state into a fresh Family, in readable and
  protected forms, including metadata, unknown fields, and saved pending
  work. No original shared access or credentials are restored. Corrupt files
  and interrupted restores leave existing Families intact.
- Importer fixtures from real, anonymized exports.

## Record scopes

One operation log covers explicit family metadata, child metadata, and child
activity scopes. Only activity records require a child ID and start time;
child metadata has its own child ID, and family metadata has neither. All
scopes share operation identity, field merge, unknown-field preservation,
encryption, and sync rules. Deleting a child tombstones its metadata and
prevents new activity targeting it, but does not cascade-delete historical
activity; restore is a new metadata operation. The UI can still export or
inspect that child's locally held history.

## Open questions

1. **M1 importer mapping.** Write source-specific field and unit maps after
   collecting real anonymized samples. This does not change M0 operation,
   identity, or portable-file bytes: imports already use stable source IDs
   where present and content identity with a preview warning otherwise.
2. **M0 execution.** Run the [portable-file contract](../protocol/portable-file-v1.md)
   and vectors across platforms. This is implementation validation, not an
   unresolved format decision. CSV remains an analysis export.

## Reconsider if

- Users ask for custom event types: add a generic `custom` type with a user
  label rather than new built-in types.
- A type needs a field that must change atomically with another: merge them
  into one field.
