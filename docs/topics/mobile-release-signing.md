# Mobile release signing

Status: early internal preparation; no public distribution. The
[MVP plan](../mvp-plan.md#milestones) owns scope and
[012](../tactical/012-internal-release-preparation.md) owns execution status.

## Android

Compile and target API 36 for new Play submissions under the
[August 2026 target requirement](https://support.google.com/googleplay/android-developer/answer/11926878?hl=en).
The minimum supported device API remains 26. SDK target compatibility is a
separate concern from native ABI and signing verification.

Use Google Play App Signing with a Google-generated app-signing key. Keep a
separate project-specific upload key in a PKCS#12 keystore. CI signs the AAB
with the upload key; Play signs the installable APKs with its app-signing key.
Upload-key replacement is recoverable through Play's reset process. The
Google-generated app-signing private key cannot be exported; future F-Droid
or other independently signed distributions need their own signing/update
plan. They cannot silently replace Play installs with a different key.

The manual `android-internal.yml` workflow runs only from `main`, builds and
tests without signing secrets, then signs in a separate job. GitHub
environment `android-internal` must restrict branches to `main` and contain:

- `BABYTRACK_UPLOAD_KEYSTORE_BASE64`: base64 PKCS#12 upload keystore.
- `BABYTRACK_UPLOAD_PASSWORD`: store/key password, identical for PKCS#12.

The alias is `upload`. CI passes passwords through environment variables,
cleans the temporary keystore, and retains only the signed AAB, public
certificate, checksum, and source/version receipt as the final artifact.
It has no Play publishing credential and makes no store upload or rollout.
GitHub secret values are not a recovery backup. Private paths and creation,
backup, restoration, and secret-provisioning commands live in the owner's
private operational inventory, not this repository.

Supply an unused increasing `version_code` and a `version_name` on dispatch.
Gradle validates their format/range. It cannot determine which codes Play
already consumed; the operator checks Play before choosing the next code.
Version properties also support local builds (see the Android README).

`scripts/sign_android_bundle.sh` requires an unsigned AAB with both supported
Rust ABIs, refuses to overwrite existing outputs, signs with JDK jarsigner,
and strictly verifies against the upload keystore/alias. Its regression
test uses disposable credentials and proves altered archives are rejected.
Signature verification is not a substitute for Android UI/device validation
or Play's server-side bundle checks. This workflow's unit/lint checks
supplement, rather than replace, the normal scaffold/relay CI gates.

## iOS

Prefer Xcode Cloud and automatic signing after the M3 app target exists.
Apple manages cloud distribution certificates/provisioning in that route;
there is no Android-style upload keystore to generate. An App Store Connect
API key authenticates API/upload operations but does not itself sign code.
External CI with manual signing needs a distribution identity and matching
provisioning profile. Do not create Apple credentials until the actual build
route needs them. The current Swift FFI smoke is not an iOS app archive.

Reconsider this choice if one CI provider must build both platforms, or
cross-store Android updates must use the same signing identity.

Sources verified 2026-10-03:
[Android signing](https://developer.android.com/studio/publish/app-signing),
[Apple cloud-managed certificates](https://developer.apple.com/help/account/certificates/cloud-managed-certificates),
[Apple signing identities](https://developer.apple.com/documentation/xcode/sharing-your-teams-signing-certificates),
[GitHub environments](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments).
