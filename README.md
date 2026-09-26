# babytrack

A planned free, MIT-licensed baby tracker with local-first logging and
opt-in sharing that is always end-to-end encrypted. Planned clients include
native phone/watch apps and a web client.
`babytrack` is a working name.

**Status: M0 implementation in progress.** The Rust core has a strict
canonical CBOR codec, structural operation decoder, tested hash, signature,
AEAD, and HPKE primitives, plus a signed encrypted batch byte proof in Rust,
JavaScript, Swift, and Kotlin. It has in-memory local/shared projections, a
native SQLite and browser IndexedDB local-only journals, and a per-Family
HLC. The development relay can commit a signed Family genesis, its first
manager-issued invitation, recipient claim, holder challenge, pending
key proof, and admission grant, then serve role-limited authenticated
control/object reads. It has no repair, removal,
or batch sync routes yet. No complete application or sharing flow
exists yet.
Scenario files
still describe expected behavior; they are not passing product tests.

## Product and architecture

The planned logging features cover feeding, sleep, diapers, pumping, growth,
medication, temperature, and notes; see the
[event model](docs/topics/event-model.md) for scope and remaining choices.

Each Family is an independent space for children, entries, and sharing.
Local logging and Family sharing work without an account. Each device has its
own Family grant; a future license or paid-service account has no Family
authority. People can join several Families, retain locally held data after
shared access ends, and continue with an independent copy. Invitations permit
one direct join without a second manual approval; joining, syncing, and
confirming shared access changes require connectivity.

Readable exports support analysis elsewhere. Full file backups, optionally
password-protected, restore saved data into a new local Family; they do not
restore access to the original group. A remaining manager can invite a lost
device's replacement. Detailed behavior and remaining choices live in the
[Family sharing contract](docs/topics/family-sharing-and-trust.md).

One Rust core owns the model, storage, merge, crypto, sync, and import/export.
Native clients own UI and platform adapters. The server relays encrypted
data without reading entries. Android is the first product UI; publishing
and hosting wait until M5.

This is the current overview. The linked plan and topics own requirements;
the original proposal is preserved as background and may contain unresolved
ideas or broader claims, not additional commitments.

## Start here

Agents read [AGENTS.md](AGENTS.md), this overview, and the two small indexes.
Then follow the task-specific links; do not load all documentation by default.

| Work | Read |
|---|---|
| Find the owning decisions | [Topic index](docs/topics/README.md) |
| Find current work and gates | [Tactical index](docs/tactical/README.md) |
| Follow the active delivery handoff | [Delivery tracker](docs/tactical/004-delivery-tracker.md) |
| Scope, milestones, cross-cutting changes | [MVP plan](docs/mvp-plan.md) |
| Run independent security reviews | [Security review runbook](docs/security-review-runbook.md) |
| Directory structure and build preparation | [Repository layout](docs/topics/repository-layout.md) |
| User-flow coverage | [Scenario index](docs/scenarios/README.md) |
| Business rationale and market background | [Background proposal](docs/product-proposal.md) |

## Repository today

`docs/topics/` holds decisions; `docs/tactical/` holds work plans;
`docs/scenarios/` holds symbolic acceptance cases. The workspace has byte
decoding, crypto, projection, local SQLite journal, and clock behavior in
`core/`. `core-wasm/` includes the browser local journal adapter;
and `core-ffi/` expose initial JavaScript, Swift, and Kotlin bindings;
`server/` has narrow genesis and first-invitation development routes; `cli/` remains a
scaffold target. `wire/` holds CBOR and public cryptographic primitives
shared between core and relay.
Only the client-facing targets depend on `core/`. The
[layout topic](docs/topics/repository-layout.md) maps future components;
[002](docs/tactical/002-repository-scaffold.md) records scaffold validation.

Install the pinned Rust toolchain through rustup, including the Clippy,
rustfmt, and `wasm32-unknown-unknown` components/target named in
`rust-toolchain.toml`. Python 3 is needed for the dependency-boundary check.
From the repository root, run:

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
```

Install `wasm-bindgen-cli` 0.2.127 and Node.js for the wasm smoke,
Java 17 for the Kotlin Gradle smoke, and `cargo-deny` 0.20.2 for the
last command. Swift is checked on macOS. The browser smoke uses Playwright's
isolated Chromium; install it with `npx playwright install chromium` from
`tests/browser/`. The Rust core's
`core/tests/cbor_vectors.rs` runs the six CBOR cases in
`tests/vectors/records-v1.json` plus canonicality and limit checks;
`core/tests/operation_vectors.rs` checks selected operation bytes and binding
failures. `core/tests/crypto_vectors.rs` checks hash, Ed25519, and XChaCha
known answers and tampering. `core/tests/hpke_vectors.rs` opens the fixed HPKE
ciphertext and tests sender round trips. `core/tests/batch_vectors.rs` matches
every byte of `BATCHBYTE01` and opens the result. Run them with
`cargo test -p babytrack-core`. The other vectors, bindings, and product
flows are not executable yet; compilation does not establish protocol
correctness.

Reference repositories are listed in [references.yaml](references.yaml).
Run `scripts/sync_references.py [name ...]` to populate gitignored
`references/`. License and reuse constraints are in `AGENTS.md`.
