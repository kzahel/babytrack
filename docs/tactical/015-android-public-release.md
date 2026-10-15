# 015: Android public release

Status: active, opened 2026-10-15 at the owner's request to publish a new
Android version publicly and automate publication through CI. The
[MVP plan](../mvp-plan.md#milestones) owns this Android scope exception;
[mobile release signing](../topics/mobile-release-signing.md) owns the
publishing contract. [012](012-internal-release-preparation.md) retains the
first internal release record.

## Goal and exclusions

Build version 0.1.1 (code 2), configure app-scoped keyless Play access, finish
first-production setup, and submit the new version for public distribution.
Keep `org.babytrack.app` and the existing signing/update identity. This work
does not deliver iOS, watches, F-Droid, or close all M5 milestones.

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
  requested version 0.1.1, code 2; result pending.
- Live Play dashboard: draft app, production inactive, 0 of 11 initial
  setup tasks complete. The existing internal release does not establish
  first-production readiness. No Play publisher credential existed in CI.
- Local publisher transaction tests (9), workflow actionlint, workspace
  boundaries, and diff checks passed. Live authentication/upload remains
  unverified until access provisioning completes.
