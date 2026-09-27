# User-flow scenarios

These files are planning artifacts, not wire formats or executable tests.
`agreed` means a specified behavior obligation; `proposed` means an open
choice in the owning topic. Neither means implemented or passing. M-1
settles the decisions; M0 binds them to concrete fixtures and assertions.

## Find the cases for your task

The [sharing catalog](family-sharing-scenarios.json) uses stable FS IDs.
Read the relevant cases and their owning decisions, not the whole catalog.

| Concern | Case IDs | Decision owner |
|---|---|---|
| Offline start and first sharing | FS01-FS03, FS49 | [Family sharing](../topics/family-sharing-and-trust.md) U1-U4; [sharing protocol](../protocol/sharing-v1.md#promotion-isolation-and-trust-limits) |
| Roles, device grants, and competing access changes | FS04-FS12, FS52, FS64, FS66-FS70 | Family sharing U3-U4, D1, D4-D5; [sync authority seam](../topics/sync-and-encryption.md#proposed-implementation-seam-for-general-relay-authority) |
| Pending work and independent copies | FS13-FS18, FS34, FS48, FS53-FS54, FS57, FS63 | Family sharing U1, U5, D7; [sharing protocol](../protocol/sharing-v1.md#verified-removal-and-private-copy) |
| Invitations, redemption, and automatic handoff | FS19-FS21, FS35-FS39, FS47, FS51, FS55, FS59, FS65, FS76-FS77 | Family sharing D2, D8; [sharing protocol](../protocol/sharing-v1.md#invitation-proof-and-admission) |
| Family/child targeting | FS22-FS25, FS54 | Family sharing U6, D7 |
| Recovery and file restore | FS26-FS30, FS40-FS45, FS61 | Family sharing U7, D3-D5; [portable file](../protocol/portable-file-v1.md) |
| Malicious relay limits | FS31-FS32, FS46, FS56 | Family sharing trust limits; [sync threat model](../topics/sync-and-encryption.md#threat-model) |
| Batch authorship and hostile data | FS50, FS60, FS62-FS63, FS67, FS69-FS70 | [Sharing protocol](../protocol/sharing-v1.md#signed-encrypted-batches-and-receipts); [records](../protocol/records-v1.md#merge-and-clock-rules) |
| Credential and Family isolation | FS52, FS58, FS64 | [Sharing protocol](../protocol/sharing-v1.md#promotion-isolation-and-trust-limits) |
| Displaced edits | FS33 | Family sharing D6; [event model](../topics/event-model.md) |

For example, read one case from the repository root:

```sh
python3 -c 'import json; d=json.load(open("docs/scenarios/family-sharing-scenarios.json")); print(json.dumps(next(c for c in d["cases"] if c["id"] == "FS27"), indent=2))'
```

The smaller [lifecycle catalog](family-lifecycle.json) contains original
smoke invariants; [projection cases](record-projection.json) describe log/view
consistency. Preserve their IDs. Where a case overlaps a newer FS scenario,
M0 may reuse its executable fixture and assertions while reporting both IDs;
maintain one implementation, not two subtly different behavioral definitions.

## Placement and handoff

The old `spec/` contained these symbolic descriptions. Concrete versioned
wire/API/file contracts live under [docs/protocol/](../protocol/README.md),
with exact cross-language input/output fixtures under
[tests/vectors/](../../tests/vectors/README.md).
See the [repository layout](../topics/repository-layout.md).

M0 gives agreed cases concrete action bindings and asserts every expected
observation. Resolve proposed cases at their design gates; do not silently
skip them and claim complete coverage. Preserve IDs in regression tests and
selected platform UI flows. Follow the [MVP review gates](../mvp-plan.md#security-review-gates).

## Coverage and execution guidance

Default scenario assumptions: an honest relay, one device per named person,
an initially consistent membership view, and no other concurrent actions,
unless the case or variant says otherwise. A lost response is distinct from
a failed commit. Each variant starts from a fresh fixture. Expand secondary
variation combinations in M0; do not infer that a list already exercises
their Cartesian product. Cases FS31-FS32 and FS46 deliberately change relay
trust.

| Product promise | Implementation responsibility | Scenario examples |
|---|---|---|
| Local work survives | Core storage, outbox, projection, copy/restore/promotion transactions | FS01, FS13, FS16, FS48-FS49 |
| Shared access changes coherently | Verified membership history and relay commit boundary | FS04-FS12, FS21 |
| Continuing privately is independent | New Family identity/keys, preserved local work, explicit UI destination | FS13-FS16, FS34, FS53-FS54 |
| Family/child context stays correct | Core scoping and platform action adapters | FS22-FS25 |
| Recovery claims match reality | Backup format, device invitation, saved-point status UI | FS26-FS30, FS40-FS45 |
| Accepted limits are honestly represented | Threat model, freshness/status wording, adversarial fixtures | FS17, FS31-FS32, FS46 |

The sync and event topics must map these obligations to concrete rules before
M0 depends on them. Fixture authoring must not invent missing authority,
recovery, or record semantics.

Cover meaningful combinations, rather than promising every unbounded
permutation of devices and network messages:

| Dimension | Required variations |
|---|---|
| People and roles | Member, one manager, two managers, two managers plus an unaffected member |
| Authority conflict | Remove/remove, remove/demote, demote/demote, last-manager leave, invite during removal |
| Connectivity | Both online, either offline, both offline, reconnect in either order |
| Delivery | Either commit order, duplicate request, lost response, delayed notice |
| Local work | No pending edit, pending new entry/edit/delete, running timer |
| Durability | Restart before request, after remote commit before response, during private copy or restore |
| Scope | One Family, multiple Families, phone/widget/watch target, third caregiver |
| Recovery | Another authorized manager can invite a device, member can copy locally held data into a new Family, readable or protected file backup restores into a new Family, no saved records |

M0 should execute these as short scripted user actions against the core and
real relay, without a product UI. Inject network delivery and clocks; never
wait for real expiry times. Compare local observations, shared membership,
record sets, and available actions. Include both schedules of each authority
race and crash boundaries explicitly; use pairwise coverage for secondary
dimensions and longer generated sequences nightly. Keep reproducible traces.

Add a smaller set of UI tests from M1 onward using the same IDs: verify the
message/state and available next action, including target Family/child.
Do not run the entire permutation set through slow UI automation. Bindings
still run their cross-language vectors independently. A core-only test does
not prove the screen communicates the result correctly.

Proposed M0 harness budget: the bounded scenario suite completes within 60
seconds on its designated CI runner, excluding build/setup. Measure before
making that a hard gate; if unrealistic, revise the budget explicitly without
dropping security or loss-prevention cases. Report scenario IDs, expanded
variation counts, failures, and unresolved proposals. M-1 adds no runner.
