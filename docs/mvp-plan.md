# MVP implementation plan

Status: agreed direction, September 2026. Companion to the proposal in
[README.md](../README.md). Open decisions are listed at the end.

## Stack

One Rust core shared by every client, with native UI on each platform.

- **Core (Rust).** Data model, operation log, merge, crypto, sync client,
  import and export, and later the voice parser. Exposed to Swift and Kotlin
  through UniFFI and to the web through wasm-bindgen. This is the pattern used
  by Mozilla application-services, Bitwarden, and libsignal. Chosen over Kotlin
  Multiplatform for mature crypto, clean wasm, and small binary cost.
- **Data model.** Append-only operation log rather than a general CRDT
  library. Every event has a UUIDv7 id. Edits are field-level operations
  stamped with a hybrid logical clock (HLC), merged last-writer-wins per field.
  Deletes are tombstones. Volume is tiny (about 30 events a day, roughly 22K
  events over two years), so each client folds the whole log in memory in
  milliseconds. Storage is only an op log plus occasional snapshots: a file on
  native, IndexedDB on web.
- **Crypto.** Op batches are encrypted with a per-epoch symmetric key using
  XChaCha20-Poly1305. Each device, platform backup, and recovery phrase is a
  key holder with its own keypairs, and new epoch keys are granted to each
  holder with HPKE. A per-epoch auth key derived with HKDF authenticates
  requests, so the server never holds a key that decrypts anything. The
  invite QR code or link carries the current epoch key. Details in
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
core/          Rust: model, ops, merge, crypto, sync client, import/export
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

Agreed September 2026. The first group shapes the sync protocol and must be
in place in M0, because changing it after clients exist is expensive.

### Protocol (M0)

The designs that meet these requirements are owned by
[topics/sync-and-encryption.md](topics/sync-and-encryption.md) and
[topics/event-model.md](topics/event-model.md).

- **Key epochs and caregiver removal.** Family keys are versioned by epoch
  and every encrypted batch records the epoch it was sealed with. Removing a
  caregiver creates a new epoch key and re-grants it to the remaining
  devices. Old epochs stay readable to members who hold them. The removal UI
  can be minimal at first, but the protocol supports it from day one.
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
- **Key backup and recovery.** The family key is backed up automatically by
  the platform's end-to-end encrypted store: iCloud Keychain (synchronizable
  item) on iOS and Block Store on Android. Every other caregiver device is
  also a backup. The 24-word recovery phrase is optional, offered at setup
  and prompted again after the first week of data. The later F-Droid build
  has no Block Store, so it prompts for the phrase more firmly.
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

- **Core.** Property tests for merge convergence: random operations applied in
  random orders across several replicas must reach identical state. This is
  the most important test in the project. Crypto known-answer tests. Fuzzing
  of decoders and importers.
- **Cross-language vectors.** Inputs and expected outputs in `spec/`, run from
  the Rust, Swift, Kotlin, and TypeScript test suites to catch binding bugs.
- **Sync torture test.** A real server and many simulated CLI clients going
  offline, reconnecting, and editing concurrently, checked for convergence. A
  second test writes known marker strings into events and fails if they appear
  in plaintext in the server database.
- **Importers.** Real Nara, Huckleberry, and Nighp exports with personal
  details removed, collected from volunteers.
- **UI.** Screenshot tests (swift-snapshot-testing on iOS, Roborazzi on
  Android). A few end-to-end flows (XCUITest, Compose tests). Playwright on
  web, including two browsers syncing through a real server.
- **Budgets in CI.** App size under 30 MB and cold start time, measured with
  Macrobenchmark on Android and XCTest metrics on iOS.
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
macOS runners are free. Every job is path-filtered.

- **core:** fmt, clippy, tests, property tests, wasm build, `cargo-deny`.
- **server:** tests against SQLite and Postgres, Docker build, image published
  to GHCR on tags.
- **android:** build, unit tests, lint, screenshot tests, and the service
  boundary check. Emulator tests nightly. Debug APKs attached as build artifacts for
  dogfooding.
- **ios:** build and unit tests on the simulator. UI tests nightly.
- **web:** lint, type-check, Vitest, Playwright.
- **e2e:** sync torture test and two-browser Playwright sync on every PR.
- **release (M5):** fastlane uploads to TestFlight and Play on version tags,
  and a reproducibility check for the F-Droid build.
- **dependencies:** Dependabot version and security updates, configured in
  `.github/dependabot.yml`, which lists every planned manifest directory.
  Update it when the layout changes.

## Milestones

M1 to M4 use developer builds on physical devices only. Nothing touches an
app store until M5.

1. **M0, foundation.** Protocol spec, core, server, and CLI. Property tests
   and the sync torture test pass. The dev server runs on a laptop and is
   reachable from phones over the LAN or Tailscale. No UI.
2. **M1, Android on our own phones.** Logging screens, timers with an
   ongoing notification, widgets, invites, recovery phrase, Nara import.
   Google services behind swappable interfaces. Installed directly and used
   daily against the dev server.
3. **M2, web.** Mostly UI over the wasm core.
4. **M3, iOS on our own devices.** Parity with Android, including Live
   Activities, run from Xcode. Free provisioning profiles expire after 7 days
   and do not include push notifications, so this milestone likely needs the
   paid Apple Developer Program. Joining it does not create a store listing.
5. **M4, watches.** Wear OS first, then Apple Watch as a phone companion.
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
