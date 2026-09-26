# MVP implementation plan

Status: agreed direction, September 2026. Companion to the proposal in
[README.md](../README.md). M-1 closes design questions before M0 implementation;
see [the M-1 tactical](tactical/001-pre-m0-design.md). Open release decisions
are listed at the end.

## Stack

One Rust core shared by every client, with native UI on each platform.

- **Core (Rust).** Data model, operation log, merge, current-record
  projection, local storage schema, crypto, sync client, import and export,
  and later the voice parser. Exposed to Swift and Kotlin
  through UniFFI and to the web through wasm-bindgen. This is the pattern used
  by Mozilla application-services, Bitwarden, and libsignal. Chosen over Kotlin
  Multiplatform for mature crypto, clean wasm, and small binary cost.
- **Data model.** Append-only operation log rather than a general CRDT
  library. Every event has a UUIDv7 id. Edits are field-level operations
  stamped with a hybrid logical clock (HLC), merged last-writer-wins per field.
  Deletes are tombstones. The immutable log is the source of truth; a
  rebuildable current-record projection supports fast timeline and summary
  queries. Native clients use SQLite for the log and projection, updated in
  one transaction; web uses IndexedDB with the same logical schema. The
  projected state is not a second source of truth. At about 30 events a day
  (roughly 22K events over two years), snapshots can be added when measured
  replay time warrants them. M-1 settles private-copy transactions.
- **Family lifecycle.** A fresh install offers a local-only Family without an
  account or relay, or can join an invitation directly. The user explicitly
  turns on sharing for each local Family. One app can hold multiple Families;
  a Family is one independent space for children, entries, membership, and
  sync, not necessarily a biological or legal household. A shared Family has
  managers, who can also manage membership, and members, who can read and
  write data. Roles are per Family. Anyone with a local copy can continue
  with it independently after leaving or being removed. The exact copy and
  identity rules are settled in M-1.
- **Multiple Families.** The active Family scopes the child list, timeline,
  logging, import/export, and settings. Every operation belongs to exactly
  one Family; its log, projection, encryption epochs, membership, relay
  endpoint, and sync cursor are isolated from other Families on the device.
  Losing access to one Family does not affect another. A running timer keeps
  the Family and child selected when it began, even if the person switches
  Families. Quick logging surfaces must identify their target Family and
  child. M-1 settles the precise switcher, invite, widget, watch, and
  notification behavior.
- **Scope of merging.** Members may log events whose activity time is in the
  past; their operation time remains the time of the write. Competitor CSV
  import remains in the MVP. Automatic merging of live families, child
  identities, memberships, or operation histories is excluded. A later
  client-side import tool may copy selected records into a target family as
  new operations; the relay never interprets their contents.
- **Crypto.** Op batches are encrypted with a per-epoch symmetric key using
  XChaCha20-Poly1305. Devices have their own keypairs; platform backup and
  recovery phrase holders are recovery paths. New epoch keys are wrapped
  for each holder with HPKE. Server authorization must distinguish devices
  and roles without seeing family content; a single auth key derived from
  the shared epoch key cannot enforce manager-only membership changes. The
  invite QR code or link carries bootstrap access. Details in
  [topics/sync-and-encryption.md](topics/sync-and-encryption.md).
- **Server (Rust, axum).** A dumb relay: an ordered log of encrypted blobs per
  family. Clients pull everything since a cursor, get live updates over a
  WebSocket, and receive empty wake pushes sent directly through APNs and FCM
  (UnifiedPush in M5).
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
- **Web.** Svelte and Vite, static build, wasm core. The family key arrives in
  the URL fragment, which browsers never send to the server.
- **Dev CLI (Rust).** Exercises sync end to end before any UI exists and
  scripts importer tests.

### Repo layout

```
core/          Rust: model, ops, projection, storage, crypto, sync, import/export
core-ffi/      UniFFI bindings for Swift and Kotlin
core-wasm/     wasm-bindgen bindings for web
server/        Rust relay
cli/           Rust dev client
apps/ios/      Xcode project: iOS app, watchOS app, widgets
apps/android/  Gradle project: phone and Wear OS modules, service modules
apps/web/      Svelte client
spec/          Protocol spec and cross-language test vectors
```

## Requirements

Agreed September 2026. The first group shapes the sync protocol. M-1 settles
its design and acceptance cases; M0 implements it, because changing it after
clients exist is expensive.

### Protocol (M0)

The designs that meet these requirements are owned by
[topics/sync-and-encryption.md](topics/sync-and-encryption.md) and
[topics/event-model.md](topics/event-model.md).

- **Key epochs and caregiver removal.** Family keys are versioned by epoch
  and every encrypted batch records the epoch it was sealed with. Removing a
  member creates a new epoch key and re-grants it to the remaining devices.
  Old epochs and data already stored locally stay readable to members who
  hold them. Managers may change membership; ordinary members may read and
  write data but may not invite, remove, or change roles. Removal stops
  future sync access after the new epoch commits; it never deletes a local
  copy. A removed member can keep working in a new, independent local-only
  family and may choose to share that new family later.
- **Manager authority.** Any manager may remove or demote another manager;
  no owner hierarchy or quorum is planned. Reject a change that would leave
  a shared family with no manager. This is a cooperative-family policy, not
  protection against a malicious manager acting first.
- **Forward compatibility.** Caregivers will run different app versions.
  Clients preserve event types and fields they do not understand and pass
  them through unchanged on edit and re-encryption, so an old client never
  silently drops data written by a newer one. Every batch and the sync API
  carry a protocol version.
- **Time.** Events store a UTC instant plus the UTC offset where they were
  logged. The day boundary for daily summaries follows the offset of the
  device showing them. The exact rule for travel is written into the spec
  with test vectors.
- **Units and locale.** Amounts, weights, and lengths are stored in metric
  and converted for display. All user-facing strings are externalized from
  the first screen, even though launch is English only.
- **Key backup and recovery.** For a shared family, other caregiver devices
  and an optional 24-word recovery phrase are independent key recovery
  paths while ciphertext history remains available. iCloud Keychain
  (synchronizable item) and Android Block Store can provide convenience
  backups, but availability and successful restore are not assumed. Cloud
  backup through Block Store is enabled only when its end-to-end encryption
  check succeeds. M-1 defines the recovery guarantee and UI states. The
  later F-Droid build has no Block Store and emphasizes the phrase. For a
  local-only family, a phrase or saved key is not a data backup: M-1 must
  specify an encrypted record backup and restore path or an explicit warning
  about unbacked local history.
- **Abuse limits.** The server has no accounts, so the hosted service
  enforces per-family storage quotas and per-family and per-IP rate limits.
  App Attest and Play Integrity are added only if abuse appears, and never
  in the F-Droid build.

### Platform service interfaces (from M1)

Every vendor service is reached through an interface the app owns, so an
implementation can be swapped without touching app code. This applies on
every platform, and it is what makes the later F-Droid build a matter of
adding implementations rather than refactoring.

- **Interfaces.** Push registration and delivery, key backup, device
  integrity, and the watch link each get a small interface in a module with
  no vendor dependencies. App code depends only on these.
- **Implementations in their own modules.** On Android, M1 ships one
  implementation per interface: FCM for push, Block Store for key backup, the
  Data Layer API for the Wear OS link. Each lives in its own Gradle module and
  is wired in at a single composition point. iOS does the same with APNs,
  iCloud Keychain, and WatchConnectivity.
- **Server side.** The server's push sender is an interface too, with APNs
  and FCM implementations first and UnifiedPush added later.
- **Boundary check.** From M1, CI fails if any module other than the
  implementation modules depends on Google Play services or Firebase. This
  keeps the boundary honest long before F-Droid work starts.
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
- A written threat model that states what the server can see: which
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

- **Core.** Property tests for merge convergence: random operations applied in
  random orders across several replicas must reach identical state. This is
  the most important test in the project. Projection replay matches
  incremental updates, including edits, tombstones, and backfilled events.
  Crypto known-answer tests. Fuzzing of decoders and importers.
- **Cross-language vectors.** Inputs and expected outputs in `spec/`, run from
  the Rust, Swift, Kotlin, and TypeScript test suites to catch binding bugs.
  Start with encoding, encryption, and unknown-field preservation; include
  version skew before treating the protocol as stable.
- **Sync integration.** A bounded, deterministic test uses a real relay and
  several CLI clients on relevant PRs. It covers offline reconnect, concurrent
  edits, membership changes, and recovery. Nightly runs extend this into
  randomized multi-client and fault-injection tests, retaining failure seeds.
  A plaintext marker test fails if event strings appear in the relay database
  or logs. M0 also exercises a minimal real-browser client with IndexedDB
  through the relay, without building the web product UI.
- **Importers.** Real Nara, Huckleberry, and Nighp exports with personal
  details removed, collected from volunteers.
- **UI.** Screenshot tests (swift-snapshot-testing on iOS, Roborazzi on
  Android). A few end-to-end flows (XCUITest, Compose tests). Playwright on
  web, including two browsers syncing through a real server.
- **Budgets.** Track installed app size and cold start from M1. Measure
  startup against a baseline on fixed physical devices with Macrobenchmark
  on Android and XCTest metrics on iOS; do not fail PRs on simulator or
  emulator timing noise.
- **Accessibility.** VoiceOver, TalkBack, Dynamic Type, and a dark theme
  suitable for night use.
- **Dogfooding (M1 to M4).** Developer builds on physical devices: our own
  phones and a few friends', installed directly as APKs or from Xcode. No
  store involvement. Pass criterion: log a feed or diaper in two taps or
  fewer from the lock screen, one-handed.
- **Store beta (M5).** F-Droid, Play internal testing, and TestFlight with 20
  to 50 families, recruited from people leaving Nara.

## CI

GitHub Actions on the public repo
[kzahel/babytrack](https://github.com/kzahel/babytrack), where standard
macOS runners are free. Keep an always-running required check; selectively
run component jobs based on changed paths. Changes to shared core, bindings,
`spec/`, dependency manifests, or CI configuration run every affected job.
PR checks use read-only permissions and no release credentials. Release and
publishing jobs run only from protected tags or environments.

- **core:** fmt, clippy, tests, property tests, wasm build, `cargo-deny`.
- **server:** tests against SQLite and Postgres, Docker build, image published
  to GHCR on tags.
- **android:** build, unit tests, lint, screenshot tests, and the service
  boundary check. Emulator tests nightly. Debug APKs attached as build artifacts for
  dogfooding.
- **ios:** build and unit tests on the simulator. UI tests nightly.
- **web:** lint, type-check, Vitest, Playwright.
- **e2e:** bounded real-relay CLI and browser protocol smoke tests on relevant
  PRs from M0; long randomized sync, failure injection, and fuzzing nightly.
  Once the web UI exists, add two-browser Playwright user flows.
- **release (M5):** fastlane uploads to TestFlight and Play on version tags,
  and a reproducibility check for the F-Droid build.
- **dependencies:** Dependabot version and security updates, configured in
  `.github/dependabot.yml`, which lists every planned manifest directory.
  Update it when the layout changes.

## Milestones

M-1 is design work only. M1 to M4 use developer builds on physical devices
only. Nothing touches an app store until M5.

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
   then two CLI clients sharing through the real encrypted relay; finally
   membership, removal, recovery, multi-Family isolation, and adversarial
   reconnect cases. Add a minimal browser harness using wasm and IndexedDB
   that syncs through the same relay, so browser storage and key handling are
   exercised before the web UI. The harness is test infrastructure, not the
   product interface. M0 exits only when the versioned vectors pass in every
   language, a mixed-client exchange converges, the bounded real-relay suite
   passes in CI, and crash/restart plus removal/recovery cases pass. The dev
   relay runs on a laptop reachable from phones over the LAN or Tailscale.
   No product UI or app-store work.
2. **M1, Android on our own phones.** Deliver in usable slices. First: local
   Family creation, children, basic feed/diaper logging, timeline, Family
   switcher, and a recoverable export/backup path on one phone. Next: sharing,
   invites, offline use on two phones, recovery, removal with a private-copy
   path, and daily dogfooding against the dev relay. Then add timers with an
   ongoing notification, widgets, Nara import, and logging refinements.
   Keep Family/child context clear throughout. Google services stay behind
   swappable interfaces; install developer builds directly.
3. **M2, web.** Mostly UI over the wasm core, including multiple Families and
   the Family switcher.
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
medication, multiple children and caregivers, import and export. Voice and
paid extras come after launch.

## Open decisions

- Hosting: a small Hetzner VPS or Fly.io. Deferred to M5.
- Final name. `babytrack` is the code name for local development and is
  used for crate, package, and bundle IDs. The final name must be chosen in
  M5 before the first App Store Connect, Play Console, or F-Droid submission,
  because the bundle ID and application ID are permanent from then on. The
  display name can change at any time.
