# Topic index

Topic documents are babytrack's durable design record. Each owns one
continuing technical concern: its current decisions, alternatives considered,
validation, open questions, and what would make us reconsider. Update the
owning topic when implementation changes the system; do not leave the only
explanation in a chat, tactical, test, or commit message.

[../mvp-plan.md](../mvp-plan.md) remains the overall plan. When a topic takes
over a concern from the plan, the plan keeps a short summary and links here.

## Active topics

- [Sync and encryption](sync-and-encryption.md) — read before changing the
  operation log, merge rules, batches, keys, key holders, grants, invites,
  removal, recovery, the server API, encoding, protocol versioning, or the
  threat model.
- [Event model](event-model.md) — read before changing entities, event types
  or fields, timers, units, time zones and day boundaries, multiple
  children, importers, or the export format.
