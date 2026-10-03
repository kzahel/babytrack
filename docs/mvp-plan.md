# MVP implementation plan

Status: agreed direction, September 2026; the bounded M0 exit passed at
`4f5ef18`. [003](tactical/003-m0-foundation.md#bounded-m0-exit-at-4f5ef18)
owns the gate evidence and post-M0 coverage queue. M1 physical-phone
validation is next.
An opt-in M2 early preview may be hosted with disposable data before M5;
this is a development testbed, not the public hosted service or release gate.
Owns scope, stack, milestone sequencing, and review gates. Read the
[topic index](topics/README.md) for detailed decisions and the
[tactical index](tactical/README.md) for current work. The
[background proposal](product-proposal.md) preserves business rationale;
it is not an additional implementation contract.

## Stack

One Rust core shared by every client, with native UI on each platform.

- **Core (Rust).** Data model, operation log, merge, current-record
  projection, local storage schema, crypto, sync client, import and export,
  and later the voice parser. Exposed to Swift and Kotlin
  through UniFFI and to the web through wasm-bindgen. This is the pattern used
  by Mozilla application-services, Bitwarden, and libsignal. Chosen over Kotlin
  Multiplatform for mature crypto, clean wasm, and small binary cost.
- **Model and storage.** Append-only operations, UUIDv7 activity IDs,
  field-level merge by verified log position, HLC metadata, tombstones, and rebuildable projections. Native
  storage is SQLite; browser storage is IndexedDB. The core owns semantics
  on both. Measure replay before adding snapshots. Exact rules and open
  cases belong to [sync](topics/sync-and-encryption.md) and
  [event model](topics/event-model.md).
- **Family behavior.** Local-only by default, explicit sharing, independent
  multiple Families, device-scoped manager/member roles, offline work, and
  private copies. Local logging and sharing require no account; licensing or
  paid-service accounts, if introduced, never authorize or decrypt a Family.
  [Family sharing and trust](topics/family-sharing-and-trust.md) owns those
  promises, removal races, single-use invitations, portable backups, and
  remaining UX choices. Live Family/child/history merging is outside the
  MVP; a later CLI import may copy selected records through the shared core.
  Basic widgets and watch access remain in their planned milestones without
  a paid unlock; paid extras are later work and never gate Family authority.
- **Crypto.** Per-epoch XChaCha20-Poly1305, per-Family device identities, HPKE
  grants, and device-specific authorization. The
  [sync topic](topics/sync-and-encryption.md) owns the constructions,
  recovery design, protocol contracts, and threat model. Do not infer that
  naming primitives settles the membership or key-handoff protocol.
- **Server (Rust, axum).** A dumb relay: an ordered log of encrypted blobs per
  family. Clients pull everything since their last verified cursor. A
  WebSocket can wake connected clients when new log entries arrive; clients
  fetch and verify the log rather than treating the notification as data.
  Startup, reconnect, foreground return, and scheduled background work also
  trigger a fetch. Connected clients use bounded polling/backoff when the
  WebSocket is unavailable. Empty APNs/FCM pushes are a later latency
  optimization (UnifiedPush in M5); missing pushes delay progress but never
  determine accepted state.
  SQLite by default, Postgres optional for the hosted service. The same binary
  serves the static web client, so self-hosting is one container.
- **iOS and watchOS.** SwiftUI, iOS 17+. WidgetKit, Live Activities for
  lock-screen timers, App Intents for Siri. Rust ships prebuilt std for
  `aarch64-apple-watchos` but not `arm64_32-apple-watchos`, which older
  watches still supported by watchOS 27 need, so the MVP watch app does not
  embed the core. It sends events to the
  phone over WatchConnectivity and renders state it receives.
- **Android and Wear OS.** Kotlin and Jetpack Compose. Glance widgets, an
  ongoing notification for running timers, Wear OS Tiles and complications.
  Wear OS can embed the core since it is a normal Android target. Android is
  the primary development platform. Google services sit behind interfaces so
  a later F-Droid build can swap them out (see Requirements).
- **Web.** Svelte and Vite, static build, wasm core. Single-use invitation
  bootstrap secrets arrive in the URL fragment, which browsers do not send
  in the initial page request. The authorized join flow delivers Family keys;
  the link does not contain a reusable raw Family key.
- **Dev CLI (Rust).** Exercises sync end to end before any UI exists and
  scripts importer tests.

### Repository and preparation

The [layout topic](topics/repository-layout.md) owns the current and future
directory map and dependency boundaries. [002](tactical/002-repository-scaffold.md)
records the completed build scaffold without product behavior. M-1 still
gates implementation of unsettled protocol/data
contracts. No empty platform projects or future-directory placeholders are
required to show the intended layout.

## Requirements

The [Family sharing and trust contract](topics/family-sharing-and-trust.md)
states agreed user flows and accepted trust boundaries, with a scenario
catalog for implementation tests. It also labels remaining UX proposals;
an implementation agent must not silently decide those while building the
protocol.

Agreed September 2026. The first group shapes the sync protocol. M-1 settles
its design and acceptance cases; M0 implements it, because changing it after
clients exist is expensive.

### Protocol and data (M0)

These requirements are fixed; their detailed behavior has one owning topic.
M-1 must settle contracts and examples before the implementation depends on
those details.

| Requirement | Authoritative decision home |
|---|---|
| Epoch rotation, device/role authorization, removal, recovery, encrypted relay | [Sync and encryption](topics/sync-and-encryption.md) |
| Local/shared authority, manager races, single-use direct invites, independent copies, backup into a fresh Family | [Family sharing and trust](topics/family-sharing-and-trust.md) |
| Records, timers, metric storage, entered units, UTC instants and recorded offsets, display/day boundaries | [Event model](topics/event-model.md) |
| Unknown types/fields preserved on edit and re-encryption; versioned batches and API | [Sync encoding](topics/sync-and-encryption.md#encoding-and-versioning) and [event preservation](topics/event-model.md#unknown-types-and-fields) |
| Full file backup/restore with optional protection, readable analysis export, importer identity | [Event import/export](topics/event-model.md#import-and-export) |

- **Device loss and backup.** A manager grants each Family-specific device
  separately. A surviving manager can invite a replacement device. The MVP
  has no recovery phrase, platform key-backup grant, or account path into the
  original Family. A complete saved file restores its records into a new
  independent Family; records saved only on lost devices may be lost. See
  [Family sharing](topics/family-sharing-and-trust.md) for the guarantees.
- **Locale.** Externalize all user-facing strings from the first screen;
  launch is English only. Amounts, weights, and lengths use metric storage.
  Screen and input rules live in
  [interface design and localization](topics/interface-design-and-localization.md).
- **Abuse.** The accountless hosted relay needs per-Family storage quotas and
  per-Family/per-IP rate limits. Add App Attest/Play Integrity only if abuse
  appears, never in the F-Droid build.

### Platform service interfaces

Every vendor service is reached through an interface the app owns, so an
implementation can be swapped without touching app code. This applies on
every platform, and it is what makes the later F-Droid build a matter of
adding implementations rather than refactoring.

- **Interfaces.** Push registration and delivery, device integrity, and the
  watch link each get a small interface in a module with no vendor
  dependencies. App code depends only on these. Portable file backup uses
  user-chosen storage and never carries original-Family credentials.
- **Implementations in their own modules.** The first Android sharing flow
  uses foreground relay polling and scheduled background sync work; FCM is
  optional later work to reduce suspended-app latency. The Wear OS Data
  Layer link arrives with its watch milestone. When added, each vendor
  implementation lives in its own module and is wired at one composition
  point. iOS follows the same rule for APNs and WatchConnectivity.
- **Server side.** A future push sender uses an interface with APNs and FCM
  implementations, and UnifiedPush in M5. No push token is needed to join,
  grant, or sync.
- **Boundary check.** CI fails if any module other than an implementation
  module depends on Google Play services or Firebase once either is added.
- **Rust built from source.** The core is compiled during the Gradle build
  (cargo-ndk). No prebuilt native libraries are committed. This costs nothing
  now and is required by F-Droid later.

### F-Droid (M5)

Deferred to M5. F-Droid builds from source and rejects proprietary
dependencies.

- **Two flavors.** `foss` swaps in non-Google implementations of the service
  interfaces. `play` keeps the M1 implementations. Everything else is
  shared.
- **Push without Google.** The `foss` build receives wake pushes through
  UnifiedPush (for example with ntfy as the distributor) and syncs over the
  WebSocket while the app is open. The server gains a UnifiedPush sender.
- **Wear OS.** The Data Layer API is part of Google Play services, so the
  Wear OS companion ships with the `play` build only.
- **Reproducible builds**, so F-Droid can publish APKs signed with our key.

### Before publishing

Publishing is deliberately far off. These must be done before the first
public release, not before development.

- Final name, then bundle and application IDs (see Open decisions).
- Publishing identity (personal or company) for both stores, privacy
  policy, GDPR position, and App Store privacy labels. The server holds push
  tokens and IP addresses even though it cannot read data.
- Confirm the M-1 threat model still states what the server can see: which
  families exist, when they write, and how much.
- Web client trust: a hosted web client cannot fully guarantee E2EE because
  the server supplies the code that handles the key. Mitigate with a strict
  Content Security Policy and self-hosting, and state the limitation in the
  docs.
- Check employment agreement terms on side projects and IP.

## References

Listed in [references.yaml](../references.yaml) and cloned into the
gitignored `references/` folder by `scripts/sync_references.py`.

- **libsignal** for a Rust core with Swift, Java, and TypeScript bindings and
  the CI that builds them. **Signal-iOS** and **Signal-Android** for QR-based
  device linking, which is close to our invite flow.
- **Actual Budget**, the closest architectural match: local-first, HLC-based
  sync through a self-hostable server, optional E2EE.
- **Mozilla application-services**, **Bitwarden sdk-internal**, and
  **uniffi-rs** for Rust-core-with-bindings structure.
- **Automerge** and **Loro** to compare against the event log design.
- **Ente** for recovery key UX and key management.
- **Horologist**, **wear-os-samples**, and **Ice Cubes** for native client
  structure.

License rule: AGPL, GPL, and source-available code is read for ideas only.
Nothing from it is copied into this MIT codebase. `cargo-deny` enforces the
same for dependencies.

## Testing and validation

Validate the protocol before depending on a product UI. M-1 defines
versioned contracts, examples, and negative cases; M0 turns them into
executable tests. Keep a small deterministic suite on relevant PRs and run
long randomized or fault-injection campaigns nightly, recording failing
seeds so they can become regression tests. A test that uses only the Rust
core cannot prove that the Swift, Kotlin, or browser bindings agree.

The [scenario guide](scenarios/README.md) owns case lookup, variation
coverage, and the fast core/real-relay versus selected UI test split. Cases
are symbolic until M0 gives them executable actions and assertions.

- **Core.** Property tests for merge convergence: random operations applied in
  random orders across several replicas must reach identical state. This is
  the most important test in the project. Projection replay matches
  incremental updates, including edits, tombstones, and backfilled events.
  Crypto known-answer tests. Fuzzing of decoders and importers.
- **Cross-language vectors.** The Rust core and relay assert exact bytes for
  their own published MVP wire formats. Swift, Kotlin, and wasm call that
  shared core, so each binding runs the same fixed encrypted child event and
  negative authentication, Family, and version-skew cases through its public
  boundary. The [vector execution matrix](../tests/vectors/README.md#execution-and-applicability-matrix)
  tracks additional published cases and gaps. Server-only and symbolic cases
  do not need a duplicate platform runner; an adapter must never reimplement
  CBOR, crypto, or merge to satisfy a test gate.
- **Sync integration.** A bounded, deterministic test uses a real relay and
  several CLI clients with independent keys and local stores on relevant PRs.
  It covers offline reconnect, concurrent edits, membership changes, and
  recovery. Tests separately cut each client's network, drop wakes and
  responses, and restart clients or the relay; assertions include verified
  cursors, pending outboxes, visible stages, and private-copy destinations.
  WebSocket notifications and polling are wake paths, while cursor fetch and
  signed acceptance evidence establish state. Nightly runs extend this into
  randomized multi-client and fault-injection tests, retaining failure seeds.
  A plaintext marker test fails if event strings appear in the relay database
  or logs. M0 also exercises a minimal real-browser client with IndexedDB
  through the relay, without building the web product UI.
- **Importers.** Real Nara, Huckleberry, and Nighp exports with personal
  details removed, collected from volunteers.
- **UI.** Screenshot tests (swift-snapshot-testing on iOS, Roborazzi on
  Android). A few end-to-end flows (XCUITest, Compose tests). Playwright on
  web, including two browsers syncing through a real server.
- **Budgets.** Aim well under 30 MB and for sub-second cold start. Track
  installed app size and cold start from M1. Measure
  startup against a baseline on fixed physical devices with Macrobenchmark
  on Android and XCTest metrics on iOS; do not fail PRs on simulator or
  emulator timing noise.
- **Accessibility.** VoiceOver, TalkBack, Dynamic Type, and a dark theme
  suitable for night use.
- **Dogfooding (M1 to M4).** Developer builds on physical devices: our own
  phones and a few friends', installed directly as APKs or from Xcode. The
  early internal draft exception is tracked in [012](tactical/012-internal-release-preparation.md).
  Pass criterion: log a feed or diaper in two taps or
  fewer from the lock screen, one-handed.
- **M1 two-caregiver gate.** On two Android phones, create one Family,
  invite a second device without simultaneous app use, load the same child
  history, record and edit while each phone is offline, and converge after
  reconnect. Exercise removal with pending work and show its private-copy
  destination. Each phone can export a readable full file, restore it offline
  into a fresh Family, and explain the saved point; a missing or corrupt
  file never claims recovery. A failed background wake leaves a visible
  pending stage and resumes on the next app run.
- **Later-surface gates.** M2 repeats the Family, offline, and restore paths
  in the real browser. M3 repeats them on iOS and validates delayed wake
  when push is unavailable. M4 validates watch and widget Family/child
  targeting, including a stale action after removal. These surfaces add no
  new Family authority semantics or paid access gate.
- **Store beta (M5).** F-Droid, Play internal testing, and TestFlight with 20
  to 50 families, recruited from people leaving Nara.

### Security review gates

Use the [independent security review runbook](security-review-runbook.md) to
launch, monitor, and record the separate Daybreak Blue review at each gate.

- **Before security implementation:** review the
  [product contract](topics/family-sharing-and-trust.md) and scenarios first,
  then the protocol mapping. Resolve D4-D8 at their stated
  gates. Reviewers return concrete user-visible counterexamples.
- **Early M0:** independent adversarial review of implemented authorization,
  invitation, encryption, and rotation before dependent sync work proceeds.
- **End of M0, before real family data:** review recovery, pending writes,
  crash safety, isolation, and scenario coverage. No unresolved finding that
  breaks an agreed access or data-retention promise may pass the gate.
- **New web/watch boundary and M5 release:** review the new trust boundary
  and deployment assumptions before relying on it with real data/public use.

Reviews use a fixed revision, record assumptions, findings, dispositions,
and regression scenario IDs. A separate security-focused model session can
assist; independent human security review is recommended before public
release. Implementation agents stop for changes to security guarantees,
compatibility, destructive recovery, or product scope, while routine internal
implementation choices remain autonomous.

## CI

GitHub Actions on the public repo
[kzahel/babytrack](https://github.com/kzahel/babytrack), where standard
macOS runners are free. Keep an always-running required check. The current
foundation workflow checks the Rust, browser, native, and Android boundaries
on each push and PR; add selective
component jobs as the suites grow, based on changed paths. Changes to shared
core, bindings, `docs/protocol/`, `docs/scenarios/`, `tests/vectors/`,
dependency manifests, or CI configuration run every affected job.
PR checks use read-only permissions and no release credentials. Release and
publishing jobs run only from protected tags or environments.

- **core:** fmt, clippy, tests, property tests, wasm build, `cargo-deny`.
- **server:** tests against SQLite and Postgres, Docker build, image published
  to GHCR on tags from M5 only.
- **android:** build, unit tests, lint, screenshot tests, and the service
  boundary check. The current emulator job runs full real-relay sharing and
  file recovery on pushes and PRs, a short daily-use UI path on each push and
  PR, and the longer UI walkthrough daily or on manual dispatch. Debug APKs
  are attached as build artifacts for dogfooding.
- **ios:** build and unit tests on the simulator. UI tests nightly.
- **web:** lint, type-check, Vitest, Playwright.
- **e2e:** bounded real-relay CLI and browser protocol smoke tests on relevant
  PRs from M0; long randomized sync, failure injection, and fuzzing nightly.
  Once the web UI exists, add two-browser Playwright user flows.
- **release (M5):** fastlane uploads to TestFlight and Play on version tags,
  and a reproducibility check for the F-Droid build.
- **dependencies:** Dependabot version and security updates in
  `.github/dependabot.yml`. The current file covers Cargo and GitHub Actions;
  add later platforms as their manifests arrive.

## Milestones

M-1 is design work only. The separate 002 scaffolding plan describes build
preparation without product/protocol behavior; its scaffold is complete.
M1 to M4 primarily use developer builds on physical devices. On 2026-10-03,
the owner authorized an early exception for iOS/Android app records,
internal-release preparation, and the first CI-built Android internal test
release to the owner-selected tester list; [012](tactical/012-internal-release-preparation.md)
owns that bounded work. This does not authorize public publication or close
the physical-phone, iOS implementation, or M5 security/release gates.

0. **M-1, design closure.** Resolve the local-to-shared lifecycle, role
   authorization, protocol, recovery, event-model, and product-scope
   decisions in [the tactical](tactical/001-pre-m0-design.md), including
   multi-Family scoping and the family-selection UX. Write the threat model,
   API and data-format contracts, and adversarial examples before
   implementation. Explicitly defer decisions that do not
   affect the protocol, with a milestone and trigger for each.
1. **M0, executable foundation.** Build the protocol, core, relay, and CLI in
   ordered slices: first a minimal encode/encrypt/decrypt proof through Rust,
   Swift, Kotlin, and wasm; then local operations, projections, and storage;
   then relay authority, invitations, and rotation with an independent
   implemented-protocol review; then mixed-client sync, recovery,
   multi-Family isolation, and adversarial reconnect cases. Add a minimal
   browser harness using wasm and IndexedDB that syncs through the same relay,
   so browser storage and key handling are exercised before the web UI.
   The harness is test infrastructure, not the product interface. M0 exits
   only when core-owned MVP byte assertions and the fixed event/negative
   binding suite pass, a mixed-client exchange converges, the bounded
   real-relay suite passes in CI, and crash/restart plus removal/recovery
   cases pass. The dev
   relay runs on a laptop reachable from phones over the LAN or Tailscale.
   No product UI or app-store work is required for this gate. The bounded
   exit passed at `4f5ef18`; extended vectors, long-history hydration,
   later-platform adapters, and physical power-loss campaigns remain in
   their owning later gates.
2. **M1, Android on our own phones.** Deliver in usable slices. First: local
   Family creation, children, basic feed/diaper logging, timeline, Family
   switcher, and a recoverable export/backup path on one phone. Next: sharing,
   invites, offline use on two phones, recovery, removal with a private-copy
   path, and daily dogfooding against the dev relay. Then add timers with an
   ongoing notification, widgets, Nara import, and logging refinements.
   Keep Family/child context clear throughout. Google services stay behind
   swappable interfaces; install developer builds directly.
3. **M2, web.** Mostly UI over the wasm core, including multiple Families and
   the Family switcher. The responsive local preview and native-managed
   browser join/sync flow, including later key epochs, are in
   [006](tactical/006-m2-web.md); verified removal and private-copy recovery
   also pass a real-relay flow, as does readable file recovery into a new
   local Family. The bounded native-managed web trust review passed at
   `fa2bcee`; web-origin sharing remains open and needs its own boundary
   recheck when implemented. A disposable-data, opt-in preview may be
   deployed early for flow checks
   while public hosting remains M5.
4. **M3, iOS on our own devices.** Parity with Android, including Live
   Activities, run from Xcode. Free provisioning profiles expire after 7 days
   and do not include push notifications, so this milestone likely needs the
   paid Apple Developer Program. Joining it does not create a store listing.
5. **M4, watches.** Wear OS first, then Apple Watch as a phone companion.
   Watch logging targets an explicit Family and child.
   Still developer builds.
6. **M5, distribution.** Final name, hosted service, and everything under
   Before publishing. The F-Droid build and UnifiedPush. F-Droid inclusion,
   Play internal testing, and TestFlight betas, then public launch.

MVP features: feeding, sleep, diapers, pumping, growth with WHO percentiles,
medication, multiple children and caregivers, import and export. Freezer milk
inventory, voice, and paid extras come after launch.

## Open decisions

- Hosting: a small Hetzner VPS or Fly.io. Deferred to M5.
- Final public name remains open. The early internal draft uses the existing
  `org.babytrack.app` identity; the code name remains in crate/package names.
  Treat a registered store identity as long-lived. A later public rename
  must decide whether to retain it or create a separate app, with the
  resulting install/update migration. Display names can be changed; the
  early draft exception does not settle public branding.
