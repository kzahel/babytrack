# 015: Android public release

Status: active, opened 2026-10-15 at the owner's request to publish a new
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
5. [ ] Complete truthful listing, assets, contact/privacy information,
   access/ads/content/target-audience/Data safety/government/financial/health
   declarations, distribution countries, and pricing in Play Console.
6. [ ] Record independent public-release security review at an immutable
   revision using the [runbook](../security-review-runbook.md). Reconcile
   blockers and existing physical-phone readiness before enabling submission.
7. [ ] Validate the new signed candidate; submit the production release and
   verify Google's actual review/publication state.

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
