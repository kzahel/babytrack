# Mobile release signing

Status: internal distribution exists; the Android public preview is submitted
for Google review, tracked in [015](../tactical/015-android-public-release.md). The
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

Android releases use `android-vMAJOR.MINOR.PATCH` tags, matching the component
release convention in the owner's other repositories. Version name and Play
version code live in `apps/android/version.properties`; release notes live in
`apps/android/CHANGELOG.md`. Local Gradle property overrides remain available
for development, but CI builds the tracked version without overrides.

Add and commit the next changelog section, then run from clean, synchronized
`main`:

```sh
scripts/release-android.sh 0.1.2 --dry-run
scripts/release-android.sh 0.1.2
```

The helper requires an increasing three-part version, increments the Android
code, commits the version file, creates an annotated tag, and atomically
pushes `main` and the tag. It refuses dirty trees, existing tags, and a local
`main` that differs from the remote. Check Play for already consumed codes
before releasing; do not reuse or move published tags. Version 0.1.1/code 4
is the historical submission baseline, not a new tag or release request.

`android-internal.yml` retains its workflow filename for continuity and is
named **Android release artifacts**. A release-tag push builds and tests
without signing secrets, then signs in a separate job. It validates that the
tag matches the tracked version/changelog and the commit belongs to `main`.
A manual run on `main` can build a diagnostic candidate, but cannot publish
it to Play or create a GitHub release. GitHub environment `android-internal`
must allow the `main` branch and `android-v*` tags, and contain:

- `BABYTRACK_UPLOAD_KEYSTORE_BASE64`: base64 PKCS#12 upload keystore.
- `BABYTRACK_UPLOAD_PASSWORD`: store/key password, identical for PKCS#12.

The alias is `upload`. CI passes passwords through environment variables,
cleans the temporary keystore, and retains only the signed AAB, public
certificate, checksum, and source/version receipt as the final artifact.
It has no Play publishing credential and makes no store upload or rollout.
Tag-triggered builds also create a GitHub prerelease containing the signed
AAB, public certificate, checksum, receipt, and changelog notes. That is an
Android candidate download, not a claim of Play availability. Ordinary
branch pushes and pull requests only run checks.
GitHub secret values are not a recovery backup. Private paths and creation,
backup, restoration, and secret-provisioning commands live in the owner's
private operational inventory, not this repository.

### Automated Play publication

`android-publish.yml` is a separate manual workflow on `main`. Supply a
release tag, successful tag-triggered `android-internal.yml` run ID, its
version code, English notes, destination (`internal` or `production`), and
status (`check`, `draft`, or `completed`). Start with `check` to verify
app-scoped access: it reads tracks
and bundles in a temporary edit, then deletes that edit without uploading or
changing any release.
It consumes the existing signed bytes rather than rebuilding. It checks the
build's repository, workflow, tag-push event, success, receipt, and SHA-256.
It resolves the tag to its commit, verifies that commit belongs to `main`,
matches the tracked version/code and signed receipt, and requires a successful
complete scaffold workflow on `main` for that exact app source. Untagged
manual candidates and historical receipts without a release tag are rejected
before Google authentication. A tag push alone never changes a Play release.
The build/sign workflow and PR checks receive no publishing credentials.
All third-party action references in both release workflows are pinned to
full commit SHAs. `check_android_release_actions.py` enforces this in normal
CI and before privileged workflows proceed. Dependabot owns pin updates.

Each destination uses its own main-only GitHub environment:
`android-play-internal` or `android-play-production`. Configure the non-secret
variables `ANDROID_PLAY_WIF_PROVIDER` and `ANDROID_PLAY_SERVICE_ACCOUNT`.
Use separate Google service accounts with access only to this Play app:
view app information and testing release permission for internal; production
release permission only for the production account. Do not grant financial,
account administration, tester-management, or unrelated-app access.
Play automatically includes app-quality viewing, policy declarations, and
deep-link management with the selected release permissions; record those
dependencies in the access inventory rather than claiming they are absent.

Authenticate through GitHub OIDC and Google Workload Identity Federation;
do not create a service-account JSON key. Restrict each provider to this
repository's immutable repository/owner IDs, `refs/heads/main`, the exact
`android-publish.yml` workflow reference, and its exact environment subject.
Bind only that environment subject as Workload Identity User on its account.
Read the repository's OIDC customization API before provisioning: repositories
using immutable subjects include the owner/repository IDs in the subject's
repository segment. Match that exact format in both the condition and binding.
Concrete cloud resource identities and provisioning commands belong in the
private operational inventory. No broad project roles are required.

Production `completed` also requires environment variable
`ANDROID_PUBLIC_RELEASE_READY=true`, set only after the release/security gate
in 015 is recorded as passed. Draft preparation does not close that gate.
The publisher refuses downgrades, checksum/version mismatches, and replacing
unrelated drafts or active/halted rollouts. It can submit its own verified
draft. It validates before committing, never silently falls back to draft,
refuses to cancel an existing Google review, and does not retry an ambiguous
commit. Reconcile ambiguous outcomes in Play Console before dispatching
again. A receipt saying `submitted` establishes
API acceptance; Google review and Console/store availability must be checked
separately. First-release listing, declarations, countries, pricing, and
Google review remain Play Console requirements; CI cannot bypass them.

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
