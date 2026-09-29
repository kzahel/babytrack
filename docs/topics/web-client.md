# Web client

Status: M2 local tracking preview implemented; shared enrollment UI is next.
The [MVP plan](../mvp-plan.md#milestones) owns milestone scope and the
[Family sharing contract](family-sharing-and-trust.md) owns access promises.
[006](../tactical/006-m2-web.md) owns delivery evidence.

## Current direction

The web app follows the Android Today, History, Family, and focused capture
routes. Narrow screens use bottom navigation; tablet and desktop widths use a
side rail and a wider content column. Child, Family, local/shared, and pending
status retain their meaning across platforms. The visual palette and
localization rules follow [interface design](interface-design-and-localization.md),
with web copy in a central English catalog and locale-aware age, date, and
time display. Browser layout remains responsive rather than a fixed phone
frame enlarged on a tablet.

Each browser profile is a separate device credential. IndexedDB stores local
operations and, after enrollment, verified authority, keys, and exact pending
wire bytes. UI Family selection in localStorage contains no secret. Clearing
site data loses that installation's credentials; another browser profile
needs another invitation. A file backup will restore saved records into a
new Family, not reinstate original shared access.

The current preview creates local Families and children and logs diaper,
whole-millilitre bottle, note, and timed left/right breast feeds. The breast
timer starts or switches sides on tap, pauses on a second tap of the active
side, excludes paused time, and keeps its unsaved, target-scoped draft across
reloads in localStorage. Saving writes one atomic segment list through the
Rust core and clears the draft only after a durable append. History can edit
sides, active durations, and pauses on that same event ID. IndexedDB replays
saved operations through the Rust wasm projection after reload. The web app
does not yet expose sharing, backup, or the full Android capture set. It
labels local-only state and does not claim a relay upload occurred.

The browser enrollment adapter currently requires invitation relay origin to
equal `location.origin`. Product web and relay routes therefore share one
canonical HTTPS origin; separate hostnames would require a reviewed protocol
and client change. The hosted code can access decrypted Family data while it
runs, so compromised delivered code can compromise a browser installation.
Review CSP, served assets, key storage, and same-origin routing at the M2
security gate before relying on web sharing with real data.

## Validation and reconsideration

The local flow gate creates two Families, logs entries to the first, reloads,
switches Family, and confirms target isolation at phone and desktop widths.
The existing browser harness already covers production wasm authority replay,
durable outbox, and native/browser encrypted exchange through a disposable
relay. M2 connects that adapter to caregiver routes and tests a complete
browser join, delayed key handoff, reciprocal edits, removal/private copy,
and browser-profile loss.

Reconsider route density after tablet/desktop spot checks, and revisit an
optional local app lock if shared-computer use makes profile access confusing.
