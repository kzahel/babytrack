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
routes. Today follows the approved Lantern composition in the
[interface design topic](interface-design-and-localization.md): a now card
that opens the running timer, since-last surfaces, one-tap Wet, Dirty, and
Sleep, a day ribbon, and recent entries. Day totals come from the core
summary for the viewer's local-day window. Narrow screens use bottom navigation; tablet and desktop widths use a
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
needs another invitation. A readable or password-protected file backup
restores saved current state into a new local Family after a preview of its
save time and record count; it never reinstates original shared access.
Protected files use the shared core's Argon2id contract in wasm. The browser
passes `navigator.deviceMemory` as its memory estimate, or assumes 1 GiB
where the browser does not report one; the profile is never weakened, and an
allocation failure is reported as an export failure.

The current preview creates local Families and children, edits a child's
name, birthday, and growth sex, and logs every Android activity type from an
Add activity chooser: bottle in mL or fluid ounces, timed or entered breast
feeds, pumping with a browser-local stopwatch, solids, sleep, diaper, growth,
temperature, medication, and notes, at now or a chosen past time. The breast
timer starts or switches sides on tap, pauses on a second tap of the active
side, excludes paused time, and keeps its unsaved, target-scoped draft across
reloads in localStorage. Saving writes one atomic segment list through the
Rust core and clears the draft only after a durable append. History rows
offer Android's corrections for each type, including breast sides, active
durations, pauses, and a moved start with Keep finish or Keep start, and a
confirmed delete with Undo, always on the same event ID. IndexedDB replays
saved operations through the Rust wasm projection after reload.
Every browser create and correction is one JSON intent, such as
`{type: 'editDiaper', child, target, kind}`, passed to the core's
`web_actions::action`. Rust validates it, binds a correction to the current
target record, and builds the same bytes the native adapter would; the web
never assembles record fields itself. The snapshot carries every read-model
field, and day totals and the analysis CSV come from the core.
Sleep uses the same shared Rust start and stop operations as Android: Today
offers a sleep start when none is running for the selected child, shows a
running sleep, including one started on another device, with its start time
and elapsed clock, and stops that same event. A started sleep is a saved
record, not a browser draft, so it syncs like any other entry. The Today
summary counts with locale plural rules. Bottom-pinned form actions keep
clearance above the floating toolbar of mobile Safari.

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
types, start and stop sleep, and edit breast feeds. Writes enter the durable encrypted outbox before
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

Each web build talks to one relay. By default that is the page's own origin,
as on the same-origin Pi preview. A build hosted elsewhere names its relay at
build time (`VITE_RELAY_ORIGIN`); the browser then sends signed relay requests
there without cookies, and accepts only invitations that name that relay.
The relay answers cross-origin requests only from web origins listed at
startup in `BABYTRACK_ALLOWED_ORIGINS`, and the hosted build's
`connect-src` lists only its own origin and that relay. Since 2026-10-10 each
`main` build is also deployed as Cloudflare static assets against the
disposable-data preview relay by `.github/workflows/web-hosted.yml`. The
hosts, Cloudflare account, and token are private deployment settings kept in
the `hosted-web` GitHub environment and the owner's private runbook, not in
this repository. The real-relay browser flow runs in both same-origin and
cross-origin modes. The hosted code can access decrypted Family data while it
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
