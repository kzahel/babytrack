# Web client

Status: M2 local tracking, native-managed shared joining, verified removal,
and readable file recovery are implemented; their bounded web trust review
passed. Web-origin sharing remains open.
The [MVP plan](../mvp-plan.md#milestones) owns milestone scope and the
[Family sharing contract](family-sharing-and-trust.md) owns access promises.
[006](../tactical/006-m2-web.md) owns delivery evidence.

## Current direction

`tracker-controller.js` owns selection, coherent loaded snapshots, invitation
progress, and polling. `App.svelte` owns rendering, navigation, and form drafts.
Async publications carry a selection generation and captured Family; newer
refreshes supersede earlier ones and route disposal prevents publication.
Durable writes keep their original target, and their later completion cannot
redirect a different selection. Fragment removal follows durable remembering;
verification and storage remain in the core adapters.

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
needs another invitation. A readable file backup restores saved current
state into a new local Family; it never reinstates original shared access.

The current preview creates local Families and children and logs diaper,
whole-millilitre bottle, note, and timed left/right breast feeds. The breast
timer starts or switches sides on tap, pauses on a second tap of the active
side, excludes paused time, and keeps its unsaved, target-scoped draft across
reloads in localStorage. Saving writes one atomic segment list through the
Rust core and clears the draft only after a durable append. History can edit
sides, active durations, and pauses on that same event ID. IndexedDB replays
saved operations through the Rust wasm projection after reload.

The web app accepts a same-origin invitation in the URL fragment or a pasted
link. Opening a URL only saves the fragment and shows the history-access
warning; an explicit Join action is required before the one-use claim is
posted. It saves exact claim/proof bytes in IndexedDB before network writes,
clears the fragment from the URL, resumes pending stages after reload, and
polls while open.
An uncertain claim replays authenticated sparse controls before retrying. If
an exact saved or archived candidate appears there, the browser recovers that
commit; otherwise a newer verified head can produce a fresh candidate using
the same device keys and enrollment nonce. Claim and proof refreshes
compare-and-swap the saved row inside one IndexedDB transaction so concurrent
tabs cannot discard a candidate that may have committed. Proof candidates use
the same reconciliation rule. A relay-signed link status distinguishes
cancellation, expiry, and issuer invalidation from unsigned denial or
timeout. A signed claimed status with an uncertain saved claim remains
explicitly uncertain
because that status does not identify the claimant. Terminal status bytes are
retained and reverified after reload until the person dismisses the attempt.
Only a verified admission grant and ready replay make the Family selectable.
The joined browser can read history, add children and the current capture
types, and edit breast feeds. Writes enter the durable encrypted outbox before
upload, remain visible offline, and retry through the relay. The Family screen
distinguishes local and shared storage and gives a manual sync action.
After a signed removal, the browser checks sparse controls against its saved
public pin even though its data-log access has ended. It retains locally held
history as an archive, prevents new writes to the old shared Family, and
automatically saves one independent local Family when durable edits are
pending. A device with no pending edits may make that copy explicitly.
The copy uses the Rust portable current-state model and an atomic IndexedDB
mapping so a retry does not create another Family. Network failure alone
does not imply removal. Creating a shared Family or issuing invitations from
web and the full Android
capture set are still open.

The public IndexedDB Family row records whether the last verified pull reached
the end of the visible log. A newly admitted browser stays in a loading stage
until that happens, even if it takes several bounded pulls. A readable shared
export marks a known gap when the saved prefix is incomplete or a removal
probe identifies skipped history, and the Family screen shows the saved
cursor and gap status. A durable outbox is shown as pending immediately after
reload, and relay calls have bounded timeouts. Verified removal and shared
outbox transactions overlap on the `removed` object store, serializing the
freeze before an independent copy's snapshot. Public append and outbox rebase
transactions use the same guard, so an in-flight sync tab cannot mutate the
source after that freeze. A second tab with a stale form can still target the
old shared Family; a save after the freeze creates or reuses the independent
copy, appends the action through the Rust local model, selects that copy, and
states where the action went. Copy creation, the triggering action, its
operation ID, and a delivery ID commit in one IndexedDB transaction; retrying
the same delivery ID cannot append twice. Child and activity record IDs
survive the current-state copy, so existing child targets and breast-feed edits
remain valid in the destination.

The browser enrollment adapter currently requires invitation relay origin to
equal `location.origin`. Product web and relay routes therefore share one
canonical HTTPS origin; separate hostnames would require a reviewed protocol
and client change. The hosted code can access decrypted Family data while it
runs, so compromised delivered code can compromise a browser installation.
The bounded native-managed web trust review is recorded in
[006](../tactical/006-m2-web.md#bounded-native-managed-web-gate-at-fa2bcee).
Review any new browser-origin authority path before relying on it with real
data.

## Validation and reconsideration

The local flow gate creates two Families, logs entries to the first, reloads,
switches Family, and confirms target isolation at phone and desktop widths.
The product UI now passes a real-relay native manager/browser test of claim,
delayed challenge and grant, native child write, browser offline note write
and reload, resumed upload, native readback, and relay plaintext scan. A
second browser joins after the first and reads older history. After a
verified device removal rotates the key epoch, a third browser joins with a
later-epoch grant and recovers its earlier keys from the committed keyring.
The welcome screen accepts a readable backup when this browser profile has
no Families. A fresh-profile Playwright flow restores and reloads a saved
Family. The same test verifies signed removal, an offline pending edit, automatic
private copy, explicit copy with no pending edits, shared readable export,
restore into a new Family, and reload. The local UI smoke covers local file
restore and a corrupt file that leaves the active Family intact. M2 still
needs web-origin invitations.

Reconsider route density after tablet/desktop spot checks, and revisit an
optional local app lock if shared-computer use makes profile access confusing.
