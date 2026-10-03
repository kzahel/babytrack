# babytrack agent instructions

Start with [README.md](README.md), the [topic index](docs/topics/README.md),
and the [tactical index](docs/tactical/README.md). Read the owning topics and
active tactical for the area being changed. Read [the MVP plan](docs/mvp-plan.md)
for milestone, scope, architectural, or cross-cutting work. The background
proposal and unrelated topics/scenarios are not mandatory context.

At each security gate in the MVP plan, use the
[independent security review runbook](docs/security-review-runbook.md) to
launch and record the separate reviewer.

Each concern has one authoritative home: product promises and technical
decisions in their owning topics, scope/milestones in the plan, work status
in tacticals, exact protocol formats in `docs/protocol/`, and test
expectations in scenario/vector files. Summaries link to the owner. If two
documents contradict, reconcile them or ask; do not silently pick one.

Status: the bounded M0 foundation gate passed at `4f5ef18`; see
[003](docs/tactical/003-m0-foundation.md#bounded-m0-exit-at-4f5ef18) for
the exact CI run, independent review disposition, and post-M0 coverage
queue. The Rust core, opaque relay, native bindings, browser harness, and
Android debug app exercise local tracking, encrypted join and sync, role
changes, rotation/removal, private copy, and backup recovery. The required
CI run passed, including real-relay Android UI/recovery checks. M1
physical-phone validation and broader scale/platform work remain open.
`babytrack` is a code name.

The working tree may contain concurrent human or agent changes. Do not revert,
reformat, or tidy unrelated work.

## Documentation

Read [docs/topics/README.md](docs/topics/README.md) and
[docs/tactical/README.md](docs/tactical/README.md) before adding or
reorganizing documentation.

A topic in `docs/topics/` is a durable record of one technical concern:
decision state, alternatives, validation, open questions, and reconsideration
triggers. Topic filenames are unnumbered kebab-case. The topic index lists
every active topic with a line saying when to read it.

A tactical in `docs/tactical/` is an executable workstream plan. Filenames use
a three-digit zero-padded prefix assigned in creation order, such as
`001-m0-foundation.md`; never renumber or reuse a number. A tactical defines
its goal, exclusions, ordered delivery slices, gates, and completion
condition. Update its status and checkboxes in the same change that makes
them true, and keep the tactical index status current.

## Architecture rules

- The shared Rust core owns the data model, operation log, merge, crypto,
  sync client, and import and export. Platform apps own UI and platform
  adapters only. They must not reimplement merge, crypto, event semantics, or
  storage schema.
- The server is a relay for encrypted blobs. It must never receive plaintext
  or a key that decrypts user data. Do not add server features that need to
  read user data.
- Local-first: local data features work offline and without an account.
  Joining, sync, and confirmed shared membership changes require network
  coordination under the Family sharing contract; never claim remote access
  changed merely because an offline request was made.
- The protocol requirements in the plan (key epochs, forward compatibility,
  protocol versioning, time representation) are fixed. Any protocol change
  updates the owning topics and scenarios; once present, update the exact
  contract in `docs/protocol/` and cross-language vectors in `tests/vectors/`
  in the same change. `docs/scenarios/` currently contains symbolic cases.
- Vendor services (push, device integrity, watch link, and any later key
  backup) are reached only through app-owned interfaces. Google Play
  services and Firebase may appear only in their implementation modules.
- No public app-store publishing or general hosted-service work before M5.
  The user-authorized early internal Android release work is bounded by
  [012](docs/tactical/012-internal-release-preparation.md). M2 may
  run the explicitly authorized disposable-data early preview described in
  the MVP plan; keep machine-specific deployment files in private infra.
- Keep medical advice and clinical content out of the MVP.

## References

Reference repos are listed in [references.yaml](references.yaml) and cloned
into the gitignored `references/` folder:

```sh
scripts/sync_references.py          # clone or update all
scripts/sync_references.py enfold   # just the named ones
```

- Never commit `references/` or edit a clone in place.
- This project is MIT. Copy nothing from AGPL, GPL, MPL, or source-available
  references; read them for ideas and implement independently. Code from MIT
  or Apache-2.0 references may be reused only with its license and
  attribution preserved.
- When adding a reference, verify the URL and license and record both in the
  manifest.

## Commit messages

These rules define the format when a commit is requested. They do not
authorize committing by themselves.

Work directly on `main`. Aim for an imperative subject of 65 characters or
fewer and wrap the body at 72 columns. For non-trivial commits, summarize the
motivating request and the key direction so a future maintainer understands
why the change exists.

Do not add `Co-Authored-By` trailers for models, generated-with banners, or
robot emoji. Record agent participation with one trailer:

```text
AI-Assisted-by: <harness> / <model>
```

For example `AI-Assisted-by: Claude Code / claude-opus-5-5`.

## Validation

The scaffold gate uses pinned Rust 1.92.0 with rustfmt, Clippy, and the
`wasm32-unknown-unknown` target; Python 3; and `cargo-deny` 0.20.2. Run from
the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --locked
cargo test --workspace --locked
cargo check -p babytrack-core-wasm --target wasm32-unknown-unknown --locked
bash scripts/check_wasm_smoke.sh
bash scripts/check_fixture_api_boundary.sh
bash scripts/check_native_smoke.sh
bash scripts/check_browser_smoke.sh
python3 scripts/check_workspace.py
cargo deny check advisories bans licenses sources
apps/android/gradlew :app:assembleDebug :app:assembleDebugAndroidTest --offline --no-daemon
bash scripts/check_android_relay_emulator.sh
bash scripts/check_android_two_emulators.sh # needs two running emulators
```

The M0 byte, operation, crypto, and in-memory replay components run
`cargo test -p babytrack-core`; fixture tests cover CBOR, selected operation
bytes, published unit conversions, hash, Ed25519, XChaCha, fixed HPKE
decryption, a full batch byte vector, and encrypted minor/inert batches.
Per-Family HLC boundary tests run there as well. The wasm smoke calls the
shared core through JavaScript with fixed encrypted vectors. The native
smoke calls it through Swift and Kotlin with the same selected vectors. All
four runtimes seal and open one fixed child event. `server` and `cli` now
have real-relay authority and encrypted exchange tests. The Playwright
Chromium smoke exercises wasm and IndexedDB with fixture data across reload
and rollback. Its local
journal adapter also exercises validated operation append, duplicate
rejection, Family isolation, and reload. Native SQLite tests cover local
append, shared history, and durable outbox. The current evidence and
remaining gates live in [003](docs/tactical/003-m0-foundation.md),
[004](docs/tactical/004-delivery-tracker.md), and
[005](docs/tactical/005-m1-android.md).
