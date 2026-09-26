# Event model

Status: proposed design, September 2026. Must be settled before M0 code.
Owns what gets logged: entities, event types and fields, timers, units, time
and day boundaries, multiple children, import mapping, and export. How
events are stored, merged, and synced is owned by
[sync-and-encryption.md](sync-and-encryption.md).

## Entities

- **Family.** The unit of sharing. Holds settings such as display units.
- **Child.** Name, birth date, and sex (WHO growth charts are sex-specific).
  A family has any number of children.
- **Holder.** A caregiver device or backup, defined in the sync topic. Each
  event records which device wrote it.
- **Event.** Everything logged. Children and family settings are also stored
  as events so they merge and sync the same way.

## Event envelope

Every event has:

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
- Lists and timelines show each event at the local time it was logged, with
  a marker when that offset differs from the viewing device's current one.
- A day is a calendar day in the viewing device's current time zone. Events
  are listed on the day they start. Daily totals, such as sleep, split a
  duration that crosses midnight between the two days.
- Travel and daylight saving cases are written into `spec/` as test vectors.

## Multiple children

Every event belongs to exactly one child. Logging for twins at once creates
one event per child sharing a `group` id, so each child's history stays
complete and either event can be edited alone.

## Unknown types and fields

Unknown types are shown as an entry that needs a newer app version and are
kept unchanged. Unknown fields on a known type are kept and written back
unchanged on edit. This is the forward compatibility requirement from the
plan applied to events.

## Import and export

- **Export** is JSON Lines, one event per line, with a schema version and
  base units, plus a CSV for spreadsheets. Export then import into a fresh
  family reproduces the same events.
- **Import** maps Nara, Huckleberry, and Nighp exports onto these types.
  Imported event ids are UUIDv5 values derived from the source app and the
  source row, so importing the same file twice creates no duplicates. The
  mapping tables are written once real, anonymized sample exports are
  collected.

## Validation

- Vectors for unit conversion and display rounding.
- Vectors for day boundaries across midnight, daylight saving changes, and
  travel between zones.
- Round-trip tests: every type through encode, merge, export, and import.
- Importer fixtures from real, anonymized exports.

## Open questions

1. **Freezer inventory.** The proposal lists pumping with a freezer
   inventory. Decide whether it is in the MVP. If so, stored milk is modelled
   as `stash.add` and `stash.use` events.
2. **Breastfeeding segments.** Segments are one field, so two caregivers
   editing the same feed at once keep only one edit. Accept that, or make
   each segment its own event linked by `group`.
3. **Day start.** Some families think of the day as starting at a fixed
   morning hour so night sleep is not split. Decide whether to offer a
   configurable day start.
4. **Diaper detail.** Whether to record colour and consistency in the MVP.
   Keep it out while medical content stays out.

## Reconsider if

- Users ask for custom event types: add a generic `custom` type with a user
  label rather than new built-in types.
- A type needs a field that must change atomically with another: merge them
  into one field.
