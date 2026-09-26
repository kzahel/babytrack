# Independent security review runbook

Use this procedure at the [MVP security gates](mvp-plan.md#security-review-gates).
The reviewer is a separate Daybreak Blue, high-thinking Codex session launched
through the local Yep Anywhere API. It reviews a fixed repository revision in
read-only plan mode. The implementation agent owns triage, fixes, regression
cases, and the gate record; a model verdict alone does not close a gate.

## Prepare the review

1. Read the gate in the MVP plan and its active tactical. Finish the work to
   be reviewed and commit it so the reviewer can name one immutable revision.
   Check `git status --short` and `git rev-parse HEAD`. If other work is still
   in the tree, do not present it as part of that revision.
2. Select the product promises, protocol files, scenarios, vectors, and code
   that belong to this gate. For Family access, start with
   [Family sharing and trust](topics/family-sharing-and-trust.md), then
   [sync and encryption](topics/sync-and-encryption.md), the
   [protocol index](protocol/README.md), and the
   [scenario index](scenarios/README.md). Include implementation paths and
   test commands when reviewing M0 or later code.
3. Confirm Yep Anywhere is running at `http://localhost:3400`, or set
   `YEP_URL` to its actual base URL. The local checkout is normally
   `~/code/yepanywhere`; its API source and
   `docs/testing/claude-agent-process-runbook.md` are the references if the
   API changes. Do not commit local credentials, URLs with tokens, or full
   private transcripts.

Write a prompt in a temporary file, for example
`/tmp/babytrack-security-review-prompt.md`. Replace `<SHA>` and the gate
description below with the exact target. Name the assumptions being tested;
do not silently ask the reviewer to prove guarantees beyond the agreed
[trust limits](topics/family-sharing-and-trust.md#accepted-trust-limits).

```text
Independently review babytrack at fixed commit <SHA> for <MVP gate>.
Read AGENTS.md, README.md, docs/mvp-plan.md, the owning topics,
docs/protocol/, relevant docs/scenarios/ and tests/vectors/, then the
implementation and tests for this gate. Work read-only: do not edit files,
commit, or send network requests other than normal local review tooling.

Test the user-visible promises first, then the protocol/implementation.
Model an honest ordered relay, an authorized hostile device, compromised
relay storage, and a malicious relay that may fork/withhold history or lie
about time. Distinguish promised protection from accepted limits. Check
offline and unknown-result states, crash/retry, mixed versions, recovery,
and each new trust boundary relevant to this gate.

For every substantive finding, give severity, exact file/line or contract
reference, a concrete attack or failure trace, the broken promise, and a
scenario/vector regression proposal. Separate blockers from hardening.
You may propose a different protocol if the current one fails; explain its
security argument, UX consequences, migration/versioning impact, and tests.
End with PASS or FAIL for the named gate and list the assumptions and
remaining limits. Do not treat unrun tests as evidence.
```

## Launch through Yep Anywhere

From the babytrack repository root, inspect `GET /api/queue` and launch only
one review at a time. The project ID is the base64url encoding, without
padding, of the absolute repository path. The `X-Yep-Anywhere: true` header
is required for the POST. Python 3 is already a repository prerequisite.

```sh
export YEP_URL="${YEP_URL:-http://localhost:3400}"
curl -fsS "$YEP_URL/api/queue"
python3 - <<'PY'
import base64
import json
import os
from pathlib import Path
from urllib.request import Request, urlopen

root = Path.cwd().resolve()
project_id = base64.urlsafe_b64encode(str(root).encode()).decode().rstrip("=")
prompt = Path("/tmp/babytrack-security-review-prompt.md").read_text()
body = {
    "provider": "codex",
    "model": "gpt-daybreak-blue-latest",
    "thinking": "high",
    "mode": "plan",
    "message": prompt,
}
request = Request(
    f"{os.environ['YEP_URL']}/api/projects/{project_id}/sessions",
    data=json.dumps(body).encode(),
    headers={"Content-Type": "application/json", "X-Yep-Anywhere": "true"},
    method="POST",
)
with urlopen(request, timeout=30) as response:
    result = json.load(response)
    print(json.dumps({"http_status": response.status, **result}, indent=2))
    if response.status != 200:
        raise SystemExit("Launch did not return a session ID; reconcile before retrying")
PY
```

Record the returned `sessionId`, `processId`, and `projectId` alongside the
reviewed SHA and prompt. A `202` response means the request is queued and
does not supply a reliably recoverable session ID: keep its queue ID, inspect
the queue, and reconcile manually. Do not submit the same prompt again just
because the first request is queued or times out. If the model alias is
unavailable, verify the current catalog and tell the user before substituting
a different reviewer.

## Monitor and collect the result

Poll `GET /api/sessions/<sessionId>/process` about once a minute. Verify
`provider: codex`, `requestedModel: gpt-daybreak-blue-latest`, and
`effort: high` in `process`. Completion requires `state: idle`,
`queueDepth: 0`, `liveness.activeWorkKind: none`, and
`liveness.derivedStatus: verified-idle`. A `waiting-input` or
`needs-attention` state requires inspection; a terminal provider error,
terminated/missing process, or repeated API error is not a review result.
Do not approve an unexpected tool request or relaunch automatically.

```sh
curl -fsS "$YEP_URL/api/sessions/<sessionId>/process"
```

After verified idle, fetch the normalized transcript and read the final
assistant message in `messages[].message.content`:

```sh
curl -fsS "$YEP_URL/api/projects/<projectId>/sessions/<sessionId>"
```

Inspect the actual findings, cited files, and claimed test results. Reproduce
blockers, fix them in the owning topic/protocol/code, add scenario or vector
regressions in the same change, then review a new fixed revision. In the
active tactical, record the SHA, session ID, threat assumptions, findings,
dispositions, regression IDs, and gate outcome. Link that record from the
tactical index or gate as appropriate. The
[M-1 record](tactical/001-pre-m0-design.md#adversarial-review-record) is an
example; its PASS applies only to the reviewed design revision, not M0 code.
