# babytrack agent instructions

Before planning or changing anything, read [README.md](README.md) (the
product proposal), [docs/mvp-plan.md](docs/mvp-plan.md) (the agreed stack,
requirements, and milestones), and the [topic index](docs/topics/README.md).
Read every topic the index names for the area being changed. The plan and
topics are authoritative. If a change contradicts them, update them in the
same commit or ask first.

Status: planning, before M0. No code exists yet. `babytrack` is a code name.

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
- Local-first: every feature works offline and without an account.
- The protocol requirements in the plan (key epochs, forward compatibility,
  protocol versioning, time representation) are fixed. Any protocol change
  updates the spec and the cross-language test vectors in `spec/`.
- Vendor services (push, key backup, device integrity, watch link) are
  reached only through app-owned interfaces. Google Play services and
  Firebase may appear only in their implementation modules.
- No app store, publishing, or hosting work before M5.
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

No build or test gates exist yet. Record the commands for each area here as
M0 creates them.
