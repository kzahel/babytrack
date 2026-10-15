# 015: Android public release

Status: submitted for Google review; public availability pending. Opened
2026-10-15 at the owner's request to publish a new
Android version publicly and automate publication through CI. The
[MVP plan](../mvp-plan.md#milestones) owns this Android scope exception;
[mobile release signing](../topics/mobile-release-signing.md) owns the
publishing contract. [012](012-internal-release-preparation.md) retains the
first internal release record.

## Goal and exclusions

Build version 0.1.1, configure app-scoped keyless Play access, finish
first-production setup, and submit the new version for public distribution.
Keep `org.babytrack.app` and the existing signing/update identity. This work
does not deliver iOS, watches, F-Droid, or close all M5 milestones.
Use the approved public name Lantern / Lantern Baby Tracker from the
[interface topic](../topics/interface-design-and-localization.md).

## Ordered delivery and gates

1. [x] Inspect existing workflow, signing inventory, CI, and live Play state.
2. [x] Start a new CI build/sign candidate on unchanged app source.
3. [x] Implement manual publication of an existing verified signed artifact,
   with exact-source CI checks and transactional publisher regressions.
4. [x] Configure dedicated Google identities, restricted WIF, main-only
   GitHub environments, and app-scoped Play permissions; verify live CI auth.
5. [x] Complete truthful listing, assets, contact/privacy information,
   access/ads/content/target-audience/Data safety/government/financial/health
   declarations, distribution countries, and pricing in Play Console.
6. [x] Record independent public-release security review at an immutable
   revision using the [runbook](../security-review-runbook.md). Reconcile
   blockers and existing physical-phone readiness before enabling submission.
7. [x] Validate the new signed candidate, submit the production release, and
   verify Google's actual review state.
8. [ ] Verify production availability in Console and the public Play listing.

Completion requires production availability verified in Console and the
public Play listing, with the exact source, version, signed artifact checksum,
CI runs, review disposition, and any remaining limits recorded here. A draft,
upload, or submission awaiting Google review is not public availability.

## Initial evidence

- Candidate source `7aae0a24c406923325b035a2c30998c3e0e2d8e1`.
- Full [scaffold run 38710890862](https://github.com/kzahel/babytrack/actions/runs/38710890862)
  passed on that source on 2026-10-15.
- [Build/sign run 38770582182](https://github.com/kzahel/babytrack/actions/runs/38770582182)
  passed for version 0.1.1, code 2. Downloaded receipt and checksum matched;
  strict JDK verification against the existing upload keystore passed.
  SHA-256 `4f05cd330b1d27405a3bdb33f86c669ccca0b9966f5a72a37048e95fc4183b02`.
  This candidate is superseded by the release-review fixes below and has
  not been uploaded to Play.
- Live Play dashboard: draft app, production inactive, 0 of 11 initial
  setup tasks complete. The existing internal release does not establish
  first-production readiness. No Play publisher credential existed in CI.
- Local publisher transaction tests (9), workflow actionlint, workspace
  boundaries, and diff checks passed. Live authentication/upload remains
  unverified until access provisioning completes.

## Independent public-release review

Reviewed immutable revision `9906afd1327d165130dc1b06f8d2740f3d52dd80`
in a separate detached checkout. Session
`01a14074-1759-7150-b4f2-258da3af27cf`, process
`0054750e-009a-402b-aeae-b84d1e0632ea`, requested/reported reviewer
`gpt-daybreak-blue-latest`, high effort. Completion was verified idle with
no queued work; the review checkout remained clean.

Assumptions matched the agreed honest relay, authorized hostile device,
compromised storage, and malicious relay with accepted fork/withholding/time
limits. Scope included Android sharing/recovery and the artifact-to-Play
authority boundary. Outcome: **FAIL; public release remains gated**.

Findings and disposition:

- High, FS20/FS77: release builds hid device role changes and unused-link
  cancellation behind debug guards. Remove those guards while preserving
  manager authorization and existing confirmed core actions. Add release-
  variant Compose tests; existing real-relay cancellation/restart/rejected-
  claim and UI confirmation tests continue to own transaction behavior.
- High, CI artifact authority: mutable Actions tags in secret/OIDC jobs.
  Pin all third-party dependencies in both release workflows and enforce
  immutable references in normal CI and release preflight.
- Medium, FS30: no-backup status was absent, and shared saved points hidden.
  Show explicit no-save status and completed timestamp in both cases. Show
  shared freshness as unknown rather than treating revision zero as current;
  retain local revision-based stale warnings. Add release UI regressions.
- Hardening: include the contracted test-data-only warning in both preview
  sharing opt-in and setup copy. New-major update-required UX remains owned
  by the existing forward-compatibility work before protocol v2 rollout.

The reviewer ran workspace Rust tests, nine publishing regressions, Android
debug unit tests and release lint, actionlint, workspace/diff checks. It did
not run emulator/physical-device or live cloud/store tests. The two-phone
gate and truthful first-production store/deployment prerequisites remain
open. A focused immutable-revision recheck is required after fixes.

Fix validation: all eight `ReleaseFamilySafetyTest` cases passed with actual
release `BuildConfig.DEBUG=false` and zero skips. Release lint, the complete
debug unit suite, debug app/instrumentation APK assembly, ten publisher
transaction tests, immutable-action checks, actionlint, workspace boundaries,
and diff checks passed locally. Shared backup freshness is deliberately
conservative; no shared journal revision or file format was changed.

### Focused recheck

At immutable revision `a953a4b97b13760e9760cfb473e1dca00adededd`, session
`01a14084-7260-7200-b5c0-be93bd9ad994`, process
`41b302f3-54ca-4a97-8e94-4632c0e1615a`, the same requested/reported reviewer
and high effort returned **PASS for remediation of all three code/CI
blockers**. Completion was verified idle, queue depth zero, no active work;
the detached checkout remained clean. The reviewer independently reran all
eight actual-release UI cases without skips, release lint, full debug units,
app/test APK assembly, ten publisher regressions, the action checker plus a
mutable-tag rejection probe, actionlint, workspace and diff checks.

This is not an overall public-release PASS. Cloud/Play access and live
verification, store/privacy declarations, two-physical-phone evidence, and
replacement CI/build receipts remain required. Keep the public-ready
environment variable false.

## Store and deployment preparation

- [Replacement build/sign run 38773979740](https://github.com/kzahel/babytrack/actions/runs/38773979740)
  passed for version 0.1.1, code 3 at the rechecked source above. Downloaded
  receipt and checksum matched; strict JDK verification against the existing
  upload key passed. SHA-256
  `b32f3d3f9e74ff25414ac150a31ecb43d1b7e3391ee1c37aff0d69a9235fdd14`.
  ZIP/ELF inspection confirmed exactly the two supported ABIs, each with the
  Rust core, and 16 KiB LOAD alignment in all six native libraries.
- [Full scaffold run 38773980956](https://github.com/kzahel/babytrack/actions/runs/38773980956)
  passed for that same source, including actual-release UI regressions and
  real-relay Android emulator checks.
- The main-only `android-play-production` GitHub environment exists with
  WIF/provider variables and `ANDROID_PUBLIC_RELEASE_READY=false`.
- The dedicated keyless Google account and constrained WIF pool/provider
  exist without project roles. Following explicit owner approval, the
  impersonation binding and app-only Play release permissions are active.
  Play's release permission dependencies include app-quality viewing,
  policy declarations, and deep-link management; no account-wide permissions
  are selected. Concrete identities stay in the private inventory.
- [Live CI access check 38783582538](https://github.com/kzahel/babytrack/actions/runs/38783582538)
  passed on attempt 3, with an `access_verified` receipt for code 3 and the
  exact reviewed source/build. The initial OIDC condition used the legacy
  subject format; the repository's customization API revealed immutable
  subjects. Both condition and service-account binding now match the exact
  immutable subject. The second attempt reached impersonation before IAM
  propagation completed; the third authenticated and exercised Play edits.
- [Production draft run 38784732246](https://github.com/kzahel/babytrack/actions/runs/38784732246)
  passed: Google accepted the bundle, edit validation passed, and commit
  produced `draft_saved` for code 3 with the checksum above. This establishes
  successful CI upload/validation/draft creation, not public distribution.
- The Lantern Baby Tracker store description, launcher icon, feature graphic,
  and four source-matched fictional app screenshots are saved in Play for
  review. Sharing is explicitly test-data-only. The no-ads declaration is
  saved. Reviewer access instructions explain account-free fictional local
  setup and self-generated two-device preview invitations. These changes
  have not been sent to Google for review.
- At the owner's request, published Lantern's [support page](https://graehlarts.com/lantern/)
  and [privacy policy](https://graehlarts.com/lantern/privacy.html) through the
  existing Graehl Arts Pages repository, commit `107a4d5`. Pages deployment
  [38786922768](https://github.com/GraehlArts/graehlarts.github.io/actions/runs/38786922768)
  passed; all four deployed page/style/icon resources returned HTTPS 200
  and matched source. Desktop and mobile rendering passed without overflow.
  The policy reuses the site's existing privacy contact and discloses the
  current absence of automatic relay expiry/self-service deletion, retained
  encrypted history and metadata, operator requests, and other-device copies.
  Its URL is saved in Play for review; the contact settings use the support
  page and the same email. The welcome privacy dialog and Family screen now
  include the full policy link; replacement code 4 is required. Actual-release
  safety tests and release lint passed locally. This does not close
  the remaining Data safety or public-release gates.
- Only one physical phone is currently attached. No app/data was installed,
  cleared, or replaced on it; the two-phone gate remains open.

## Final early-preview submission preparation

The owner instructed publication after the policy/site preparation. Execute
the bounded early public preview exception now recorded in the owning MVP
plan: M1 physical-phone checks remain deferred and unpassed. Sharing remains
opt-in, test-data-only, and disposable. This is not real shared-data readiness
or M1 completion; required code/security, CI, signing, store, and privacy
checks still apply.

At immutable `5191d0b161578f98685f0cfd7d68e8ceb618f5ec`, independent session
`01a140e1-3066-7013-a4bb-d25692b2ad5b`, process
`0c67c156-4748-4c26-ae01-5e44ba396989`, requested/reported Daybreak Blue at
high effort, returned **PASS for code/privacy/CI**, with no substantive
regression in the full-policy link. The reviewer independently passed all
eight actual-release safety tests without skips and release lint. Verified
idle, zero queue, no active work, clean detached checkout.

Its overall gate was **FAIL for readiness evidence**, naming physical checks,
incomplete store declarations, unfinished code-4 CI, and absent public
submission. The physical-check disposition is the explicit plan exception
above; the other evidence must be completed and recorded before enabling
submission. No model verdict is represented as public availability.

Play's initial setup tasks are now complete and saved for review: Parenting,
adult audience 18+, Everyone/PEGI 3 with Users Interact and invitation-only
sharing, no ads, no government affiliation or financial features, public
privacy/contact and reviewer access instructions. Health declaration covers
nutrition/weight, sleep, medication record keeping, and other manual infant
care records; no clinical advice, diagnosis, dosing, or medical device claim.
Data safety declares optional, retained app-interaction/device-ID metadata
for functionality (device IDs also for security), TLS, no hosted accounts,
operator deletion-request URL, and the applicable E2EE/user-initiated-transfer
exemptions. No MASA/global-standard security badge is claimed.

Production distribution is saved for 178 countries/regions, including Rest
of World. App pricing is verified Free. Replaced the store screenshots with
the same four fictional fixtures from the code-4 source's CI gallery; the
Family image now includes the test-data-only warning. The published policy
link also renders below Family options. The superseded code-3 production
draft was deliberately discarded; its uploaded bundle remains in the library.

[Code-4 build/sign 38787633603](https://github.com/kzahel/babytrack/actions/runs/38787633603)
passed for 0.1.1 at `5191d0b161578f98685f0cfd7d68e8ceb618f5ec`.
The downloaded source/run/version receipt and checksum matched; strict
verification against the canonical upload keystore passed. SHA-256:
`445128f9b3c51bb487566312aad8b9ffe89442b647c26dc29b2f1af81cdad59a`.
Both supported ABIs contain the Rust core; all six native libraries have
16 KiB ELF LOAD alignment.

[Exact-source scaffold 38787633938](https://github.com/kzahel/babytrack/actions/runs/38787633938)
passed all jobs, including the real-relay tests, caregiver correction/restart,
readable/protected file recovery across fresh installs, navigation captures,
and Required checks. No physical-phone evidence is inferred from this run.
The publisher also requests `ERROR_IF_IN_REVIEW` when committing, so a later
dispatch cannot silently cancel an existing Google review. All ten local
publication regressions pass with that explicit API behavior.

[Code-4 production draft 38792052993](https://github.com/kzahel/babytrack/actions/runs/38792052993)
passed upload, server validation, and commit with `draft_saved`, matching the
source, build run, version, and SHA-256 above. Console independently shows
code 4, 178 countries/regions, and a 13.1 MB new-install size. Its sole
non-blocking warning is the missing deobfuscation file; this build does not
enable obfuscation.

Final focused submission recheck at immutable
`0ff3ab9eb33a9a348ec7321906309c6facf0879c`, session
`01a140fb-e901-7a12-9ee5-8cdde12941b9`, process
`76ce832f-a15d-455d-863e-aab5cb6c69e1`, requested/reported Daybreak Blue,
high effort, returned **PASS for submitting this bounded preview after
successful code-4 validation**. No substantive findings or scope
contradictions. It independently passed all ten publisher tests and verified
the review-cancellation guard; unchanged Android/Rust suites were not rerun.
Verified idle, zero queue, no active work, clean detached checkout.

Implementer gate disposition: **PASS for submission**, reconciling the prior
readiness FAIL with the explicit physical-check scope exception, completed
truthful Play setup, exact-source full CI/signing evidence, and successful
code-4 server validation. Enable `ANDROID_PUBLIC_RELEASE_READY=true` for this
submission. Google review and actual availability remain downstream; this
does not mark physical checks, M1, or general real shared-data readiness passed.

## Submitted for Google review

The production `completed` attempt
[38792602006](https://github.com/kzahel/babytrack/actions/runs/38792602006)
passed provenance/authentication, then Google returned HTTP 400 at edit
validation, before commit. The uncommitted edit was discarded; no ambiguous
commit or silent fallback occurred. Console then surfaced an incomplete
Advertising ID declaration. Verified no AD_ID permission or advertising-ID
SDK in the release manifest/dependencies and saved the truthful No answer.

Finished the existing code-4 release preview in Console, preserving the
verified bundle and release notes, then submitted all 11 pending changes.
At approximately 19:21 UTC on 2026-10-15, Play confirmed **11 changes sent
for review** and **Changes in review**, including production 0.1.1/full
rollout, countries, listing, and declarations. Google's quick checks were
still running with up to 14 minutes shown; no issue remained displayed.
Managed publishing is off: publication follows successful Google checks and
approval automatically. This is a submission receipt, not live availability.

The expected public listing is
[Lantern Baby Tracker](https://play.google.com/store/apps/details?id=org.babytrack.app).
Check Console and that listing before marking step 8 or this tactical complete.
Build/sign/upload/validation/draft creation through CI are live-verified.
The `completed` API commit is configured and regression-tested but has not
yet succeeded in a live run; this first review submission was completed in
Console. Do not retry the failed completed run while this review is pending.

## Versioned release flow alignment

At the owner's request, compared yepanywhere's tag-triggered npm/component
workflows and JSTorrent's `android-v*` release workflow/helper. Adopt the
Android component convention, with Play promotion kept separate from the
tag-triggered candidate build. The current contract is in
[mobile release signing](../topics/mobile-release-signing.md#android).

- [x] Track the existing 0.1.1/code-4 baseline and Android changelog in source.
- [x] Add `scripts/release-android.sh VERSION`, including dry-run, clean/synced
  `main` checks, monotonic version/code, annotated tag, and atomic push.
- [x] Build/sign on `android-v*` pushes and create a downloadable GitHub
  prerelease; manual `main` candidates remain diagnostic only.
- [x] Require matching tag, source, tracked version/code, successful tag-push
  build, full exact-source `main` CI, receipt, and checksum before Play auth.
- [ ] Verify the first tagged build/sign/GitHub release and subsequent Play
  promotion after these workflow changes are committed and deployed.

This alignment creates no release tag, submits no new Play edit, and does not
change the code-4 submission already under review. Historical pre-tag CI
receipts remain evidence for that submission, not eligible inputs to the new
publisher. Existing main-only Play environments and Google WIF restrictions
remain the publishing boundary. The live `android-internal` signing environment
now allows the `main` branch and `android-v*` tags; API readback verified both
rules. Production Play's environment still allows only `main`; Google WIF was
not changed. The optional internal Play environment has not been provisioned.

Local checks passed: five release/authority tests, including an actual atomic
push to a disposable local bare remote and tag/source/CI rejection cases;
ten publisher transaction/receipt tests; real JDK signing/tamper regression;
actionlint; immutable-action and workspace-boundary checks; and Gradle
configuration plus release BuildConfig generation. First tagged CI execution
and Play promotion remain unverified; no release or submission was started.
