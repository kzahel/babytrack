# Event model

Status: proposed design, September 2026. M-1 must settle the record envelopes
and remaining MVP questions before M0 code.
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
- **Holder.** A caregiver device or backup, defined in the sync topic. Each
  event records which device wrote it.
- **Activity event.** A child-scoped item in the timeline. Child and family
  metadata also use the shared operation and sync machinery, but their
  envelope and scope are an M-1 decision.

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
and independent. A field whose parts must change together, such as a list of
feeding segments, is a single field.

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

Growth percentiles are computed from WHO tables when shown, never stored.
Wake-window hints are computed from sleep events on the device.
Freezer milk inventory is outside the MVP; pumping records do not imply a
stock ledger or `stash.add`/`stash.use` operations.

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
  operation's HLC records when the change was made and controls merge order.
  Backfilling does not require rewriting old operations.
- Lists and timelines show each event at the local time it was logged, with
  a marker when that offset differs from the viewing device's current one.
- A day is a calendar day in the viewing device's current time zone. Events
  are listed on the day they start. Daily totals, such as sleep, split a
  duration that crosses midnight between the two days.
- Travel and daylight saving cases are written into `tests/vectors/` as test vectors.

## Multiple children

Every activity event belongs to exactly one child in exactly one Family.
Child identity and display name are scoped to that Family. Two Families may
each represent the same real child but keep separate histories unless a user
explicitly imports records into one of them. Logging for twins at once
creates one event per child sharing a `group` id, so each child's history
stays complete and either event can be edited alone.

## Unknown types and fields

Unknown types are shown as an entry that needs a newer app version and are
kept unchanged. Unknown fields on a known type are kept and written back
unchanged on edit. This is the forward compatibility requirement from the
plan applied to events.

## Import and export

- **Export and file backup.** Readable CSV supports spreadsheets and analysis.
  A documented full structured export also serves as a restorable Family
  backup, with optional password protection. The working format is versioned
  JSON Lines with base units; the earlier one-event-per-line sketch must also
  accommodate children, required settings/metadata, and unknown fields.
  M-1 settles the exact envelope, deletion semantics, and whether full edit
  history is included. CSV is not a full-fidelity backup promise.
- **File restore.** Restore the saved record state into a new independent
  local-only Family with fresh identity and keys, offline and without the
  original group's permission. Include saved pending local work in the
  snapshot. Existing Families remain unchanged. Files contain data, not
  original-Family device credentials or authority to rejoin. The user can
  share the new Family and invite caregivers again. Protected and readable
  forms must restore equivalent saved data; the protected form also requires
  its protection credential. See product decision D3.
- **Import** maps Nara, Huckleberry, and Nighp exports onto these types.
  Imported event ids are UUIDv5 values derived from the source app and the
  source row, so importing the same file twice creates no duplicates. The
  mapping tables are written once real, anonymized sample exports are
  collected.
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
  retains the current HLC for merge ordering.
- Round-trip tests: every type through encode, merge, export, and import.
- Full backup restores saved state into a fresh Family, in readable and
  protected forms, including metadata, unknown fields, and saved pending
  work. No original shared access or credentials are restored. Corrupt files
  and interrupted restores leave existing Families intact.
- Importer fixtures from real, anonymized exports.

## M-1 candidate: record scopes

Use one operation log over records with an explicit scope: family metadata,
child metadata, or child activity. Only activity records require a child id
and start time; child metadata has its own child id, and family metadata has
neither. All scopes retain the same id, field-level merge, unknown-field,
encryption, and sync rules. M-1 must confirm this envelope and define how
deleting a child affects its activity history before encoding is frozen.

## Open questions

1. **Breastfeeding segments.** Segments are one field, so two caregivers
   editing the same feed at once keep only one edit. Accept that, or make
   each segment its own event linked by `group`.
2. **Day start.** Some families think of the day as starting at a fixed
   morning hour so night sleep is not split. Decide whether to offer a
   configurable day start.
3. **Diaper detail.** Whether to record colour and consistency in the MVP.
   Keep it out while medical content stays out.
4. **Metadata envelope.** Decide whether family settings and children are
   distinct record types with their own fields, while sharing the operation
   log and merge machinery. They cannot use the required child id and
   activity start time above without inventing false values.
5. **Import identity.** "Source app and source row" must mean a stable
   source record identifier or content-derived identity, not a line number:
   overlapping exports can reorder rows. Specify how repeated imports and
   genuinely changed source records behave.
6. **Full backup format.** D3 decides portable file restore into a new local
   Family with optional protection. Specify JSON Lines envelopes and versions,
   required metadata, unknown fields, units, deletion semantics, and whether
   full edit history is included beyond the required saved record state.
   Specify the protection wrapper and validation/failure behavior. CSV remains
   an analysis export, not a substitute for the full backup contract.
7. **Daily reports while travelling.** Current-viewer time zone makes two
   caregivers in different zones see different daily totals. Decide whether
   that is intended or whether family reports use a stable family zone.

## Reconsider if

- Users ask for custom event types: add a generic `custom` type with a user
  label rather than new built-in types.
- A type needs a field that must change atomically with another: merge them
  into one field.
