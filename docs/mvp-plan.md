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
- **Crypto.** One symmetric family key. Op batches are encrypted with
  XChaCha20-Poly1305. A separate auth key derived with HKDF from the family
  secret authenticates writes to the server, so the server never holds a key
  that decrypts anything. The invite QR code or link carries the secret.
  Recovery is a 24-word phrase. Removing a caregiver rotates to a new family
  key and re-invites the rest, which is acceptable for the MVP.
- **Server (Rust, axum).** A dumb relay: an ordered log of encrypted blobs per
  family. Clients pull everything since a cursor, get live updates over a
  WebSocket, and receive empty wake pushes sent directly through APNs and FCM.
  SQLite by default, Postgres optional for the hosted service. The same binary
  serves the static web client, so self-hosting is one container.
- **iOS and watchOS.** SwiftUI, iOS 17+. WidgetKit, Live Activities for
  lock-screen timers, App Intents for Siri. Rust ships prebuilt std for
  `aarch64-apple-watchos` but not `arm64_32-apple-watchos`, which Series 6 to 8
  need, so the MVP watch app does not embed the core. It sends events to the
  phone over WatchConnectivity and renders state it receives.
- **Android and Wear OS.** Kotlin and Jetpack Compose. Glance widgets, an
  ongoing notification for running timers, Wear OS Tiles and complications.
  Wear OS can embed the core since it is a normal Android target.
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
apps/android/  Gradle project: phone and Wear OS modules
apps/web/      Svelte client
spec/          Protocol spec and cross-language test vectors
```

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
- **Beta.** TestFlight and Play internal testing with 20 to 50 families,
  recruited from people leaving Nara. Pass criterion: log a feed or diaper in
  two taps or fewer from the lock screen, one-handed.

## CI

GitHub Actions. Standard macOS runners are free for public repos, so the repo
should be public on GitHub before CI work starts. Every job is path-filtered.

- **core:** fmt, clippy, tests, property tests, wasm build, `cargo-deny`.
- **server:** tests against SQLite and Postgres, Docker build, image published
  to GHCR on tags.
- **android:** build, unit tests, lint, screenshot tests. Emulator tests
  nightly.
- **ios:** build and unit tests on the simulator. UI tests nightly.
- **web:** lint, type-check, Vitest, Playwright.
- **e2e:** sync torture test and two-browser Playwright sync on every PR.
- **release:** fastlane uploads to TestFlight and Play on version tags.
  Renovate for dependency updates.

## Milestones

1. **M0, foundation.** Protocol spec, core, server, and CLI. Property tests
   and the sync torture test pass. No UI.
2. **M1, iOS.** Logging screens, timers, Live Activity, widgets, invites,
   recovery phrase, Nara import. TestFlight beta.
3. **M2, web.** Mostly UI over the wasm core. Gives Android users something
   before the Android app ships.
4. **M3, Android.** Parity with iOS, Play beta. Public launch with iOS,
   Android, and web.
5. **M4, watches.** Apple Watch as a phone companion, and Wear OS. Fast follow
   after launch.

MVP features: feeding, sleep, diapers, pumping, growth with WHO percentiles,
medication, multiple children and caregivers, import and export. Voice and
paid extras come after launch.

## Open decisions

- Which phone platform first. iOS is suggested because the category's revenue
  is there, but the platform used for daily dogfooding matters more.
- Hosting: a small Hetzner VPS or Fly.io.
- Whether the recovery phrase is mandatory or optional during setup.
- Name.
