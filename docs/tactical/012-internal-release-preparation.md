# 012: Internal release preparation

Status: Android internal release delivered on 2026-10-03; iOS implementation
and uploads explicitly deferred by the owner. Opened at the owner's request
to prepare unpublished iOS and Android properties for internal testing. The
[MVP plan](../mvp-plan.md#milestones) owns this exception to the M5 schedule.

## Goal and exclusions

Prepare store records and identify/build the artifacts required for internal
testing. Retain the existing `org.babytrack.app` identity. The owner authorized
the first CI-built Android test release to the selected internal tester list.
Do not publish publicly, expand that audience, claim missing compliance
declarations, or commit signing credentials. This task does not implement the M3 iOS app or
close the physical-phone and M5 release/security gates.

## Ordered delivery slices

1. [x] Confirm authenticated access to both developer consoles and inspect
   new-app requirements.
2. [x] Register the explicit Apple bundle identifier `org.babytrack.app`.
3. [x] Verify the new App Store Connect record and prepare TestFlight setup.
4. [x] Create the Play app after the owner confirms its policy-compliance
   and encryption-export declarations. The owner explicitly approved both
   declarations before submission.
5. [x] Build and inspect the Android release bundle with only architectures
   supported by the shared Rust core.
6. [x] Create the dedicated Android upload key, install main-only GitHub
   environment secrets, and verify a locally signed bundle. Record private
   paths, creation commands, fingerprints, and recovery in the owner's dotfiles.
7. [x] Implement a manual build/sign/artifact workflow with signing regression
   coverage; keep build jobs free of credentials and publish no store release.
8. [x] Commit/push the workflow, verify hosted build/sign CI, upload its
   signed bundle, and make the test release available on the internal track.
9. Deferred to M3: supply an implemented, signed iOS app archive before a
   TestFlight upload. Keep the existing empty app record; no iOS work is
   required to complete this Android workstream.

## Evidence and limits

The first hosted run at `afd14f5` was canceled before upload when the
current Play target requirement was checked. Compile/target SDK and CI
platform installation were updated to API 36; minimum device API remains 26.

The unsigned `:app:bundleRelease` build and its release lint passed before
and after the packaging fix. Initial inspection found dependency-only ABIs
without the Rust core; the Gradle ABI filter now matches the two Rust
targets. ZIP inspection of the rebuilt bundle asserted exactly `arm64-v8a`
and `x86_64`, with `libbabytrack_core_ffi.so` present in each. Signing
certificates were absent, as expected with the current unsigned release
configuration. The workspace boundary check and `git diff --check` passed.

The [Apple app record](https://appstoreconnect.apple.com/apps/6818840568/distribution)
exists in Prepare for Submission, with bundle ID `org.babytrack.app` and SKU
`babytrack-ios`. Its `Internal` TestFlight group exists with automatic
distribution disabled, zero testers, and zero builds. There is no iOS build
to upload yet. No tester invitations have been sent.

The [Google Play app record](https://play.google.com/console/u/0/developers/7120489125876988801/app/4974715501623629403/app-dashboard)
exists as `Babytrack`, package `org.babytrack.app`, under Graehl Arts. Its
policy-compliance and US encryption-export declarations were accepted after
the owner's explicit confirmation. Apple rejected the display name
`Babytrack` as already in use; the iOS draft uses `Babytrack Internal`.

Google's internal track has release `0.1.0 internal draft` (an internal
console label) with English release notes, including feeding corrections.
The track is Active and version code 1 is available to internal testers.
The track selects the dedicated `Babytrack Internal` email list with the
single owner-provided tester account. The opt-in link is available in Play.

The [signing topic](../topics/mobile-release-signing.md) owns the upload-key
and platform-signing contract. A local `babytrack-1.aab` was signed with the
new upload key and passed strict JDK verification against that keystore.
The release build/full release lint, workflow actionlint, signing regression
with disposable credentials (including tampered archive rejection), workspace
boundary check, and diff checks passed. ELF inspection found 16 KiB LOAD
alignment for all six packaged native libraries. Private credential inventory and
commands are recorded in the owner's dotfiles runbook; GitHub environment
configuration and secret names were verified through the API.

## Hosted release evidence

- Source: `761975eab6afed7fb19d2dfd79bf34db2c1cadb8`.
- [Android CI run 37144741466](https://github.com/kzahel/babytrack/actions/runs/37144741466):
  build and sign jobs passed, artifact `babytrack-play-1`.
- Version `0.1.0`, code `1`, minimum API 26, target API 36, two ABIs.
- Downloaded AAB SHA-256:
  `05fdabaa02b451cb85bfd0d74565d689bdde244d34b3219564eb9f7748a2a138`.
  Receipt, checksum, certificate fingerprint, and strict JDK verification
  against the upload keystore all passed before upload.
- Play accepted the bundle and showed no blocking validation errors. Its
  deobfuscation warning is nonblocking; R8/ProGuard is not enabled here.
- Play internal track showed Active and Available to internal testers on
  October 3, 2026. Public distribution was not enabled.
- The independent [scaffold run 37144741102](https://github.com/kzahel/babytrack/actions/runs/37144741102)
  passed Rust, native bindings on both platforms, and Android debug build,
  but browser invitation smoke failed with `Relay control commit failed: 409`.
  The Android relay emulator job was still running at upload time. Full
  scaffold CI is not green; investigate the browser failure separately.

The bounded Android delivery is complete. iOS remains deferred to M3;
physical-phone behavior, the separate browser failure, and broader M5
security/public-release gates remain open.
