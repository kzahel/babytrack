# babytrack

A planned free, MIT-licensed baby tracker with local-first logging and
opt-in sharing that is always end-to-end encrypted. Planned clients include
native phone/watch apps and a web client.
`babytrack` is a working name.

**Status: design and repository preparation, before M0 implementation.**
The Rust workspace compiles, but it has no application or relay behavior and
no runnable product tests. The scenario files describe expected behavior;
they are not passing tests.

## Product and architecture

The planned logging features cover feeding, sleep, diapers, pumping, growth,
medication, temperature, and notes; see the
[event model](docs/topics/event-model.md) for scope and remaining choices.

Each Family is an independent space for children, entries, and sharing.
Local logging works offline without an account. People can join several
Families, retain locally held data after shared access ends, and continue
with an independent copy. Invitations permit one direct join without a
second manual approval; joining, syncing, and confirming shared access
changes require connectivity.

Readable exports support analysis elsewhere. Full file backups, optionally
password-protected, restore saved data into a new local Family; they do not
restore access to the original group. Detailed behavior and remaining
choices live in the [Family sharing contract](docs/topics/family-sharing-and-trust.md).

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
| Scope, milestones, cross-cutting changes | [MVP plan](docs/mvp-plan.md) |
| Directory structure and build preparation | [Repository layout](docs/topics/repository-layout.md) |
| User-flow coverage | [Scenario index](docs/scenarios/README.md) |
| Business rationale and market background | [Background proposal](docs/product-proposal.md) |

## Repository today

`docs/topics/` holds decisions; `docs/tactical/` holds work plans;
`docs/scenarios/` holds symbolic acceptance cases. The workspace has empty
build targets in `core/`, `core-ffi/`, `core-wasm/`, `server/`, and `cli/`.
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
python3 scripts/check_workspace.py
cargo deny check advisories bans licenses sources
```

Install `cargo-deny` 0.20.2 for the last command. All five crates currently
report zero unit and documentation tests; compilation and these checks do
not establish product or protocol correctness.

Reference repositories are listed in [references.yaml](references.yaml).
Run `scripts/sync_references.py [name ...]` to populate gitignored
`references/`. License and reuse constraints are in `AGENTS.md`.
