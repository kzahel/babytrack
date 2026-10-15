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
4. [ ] Configure dedicated Google identities, restricted WIF, main-only
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
