# Browser regression runners

The smoke uses bundled Playwright Chromium, disposable relays, and fixed
synthetic vectors. Build prerequisites and installation are in the root
README. Run every scenario with:

```sh
bash scripts/check_browser_smoke.sh
```

List names without building, or run a subset from a fresh browser context:

```sh
node tests/browser/browser-smoke.cjs --list
bash scripts/check_browser_smoke.sh --scenario candidate-races,rebase-recovery
```

`browser-smoke/` separates journal, public authority, invitation, concurrent
candidate refresh, dynamic enrollment, recipient/initial exchange, rotation,
and rebase recovery. Scenario assertions retain the shared fixture bytes.
The support module owns fixture loading, relay setup/proxy, and cleanup.
Each scenario starts with isolated IndexedDB and owns its relay processes.

Progress identifies the running scenario. Failure artifacts under
`target/browser-diagnostics/` contain the original failure, relay logs, and
a Playwright trace. Override the root with `BABYTRACK_BROWSER_DIAGNOSTICS_DIR`.
CI retains these artifacts when present. Product UI flows remain in
`web-ui-smoke.cjs` and `web-sharing-ui.cjs`; their build wrappers share the
web output directory and must run sequentially.
