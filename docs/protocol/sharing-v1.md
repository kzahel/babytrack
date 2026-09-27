# Sharing protocol v1

Status: M-1 decided contract, September 2026; implementation and cross-language
execution remain M0 work. This is a
versioned internal contract, not a deployed API. It implements the
[Family sharing product contract](../topics/family-sharing-and-trust.md).
An honest relay supplies one durable total order and atomic compare-and-swap;
clients independently verify authority and encryption. A malicious relay can
withhold or fork a valid history. Version 1 does not promise global
non-equivocation. There is no login or person-wide principal.

## Terms and cryptographic suite

`Family ID`, `device ID`, `transition ID`, `invitation ID`, `batch ID`, and
`grant ID` are distinct random 16-byte IDs. A device ID names a
Family-specific Ed25519 signing key and X25519 agreement key version. A
copied private key is the *same* device, including its role and sequence;
revoking that device revokes every copy. New installations without that key
need a fresh invitation. No file or account grants old Family authority.

The relay has an Ed25519 signing key generated for its endpoint. `relay_id`
is SHA-256 of its 32-byte public key; the Family genesis and invitation
bootstrap pin both this ID and an exact UTF-8 relay origin. An origin is
lowercase HTTPS scheme/host, optional explicit port, no path/query/fragment
or trailing slash; default port 443 is omitted. Local development may use
`http://localhost:<port>` but production clients reject plain HTTP. Endpoint
migration is not part of v1. TLS authenticates transport; the relay key signs
receipts, not client authority. A self-hosted operator who replaces the relay
key must perform an explicit new-Family migration, never silent trust reset.

Cryptographic algorithms are SHA-256, Ed25519, XChaCha20-Poly1305 with random
24-byte nonces, and RFC 9180 HPKE base mode using DHKEM(X25519, HKDF-SHA256)
`0x0020`, HKDF-SHA256 `0x0001`, ChaCha20-Poly1305 `0x0003`. Keys are random
32 bytes. A nonce must never repeat for one AEAD key; retries send identical
bytes, while rebatching allocates a fresh nonce and batch ID. No custom
cryptographic primitive is implied by the transcript composition below.

For ASCII `label`, define `H(label, bytes) = SHA256("babytrack/v1/" || label
|| 0x00 || bytes)`. Ed25519 signs the 32-byte `H(label, canonical-CBOR-value)`;
the signature is the 64-byte result of ordinary Ed25519, not Ed25519ph.
Each distinct signed object uses its own label. Byte strings and CBOR obey
[records v1](records-v1.md#canonical-values). All version-1 control maps
reject unknown keys, missing fields, noncanonical bytes, and duplicate IDs.
No app may treat a relay receipt alone as verified membership.

## Authorization state and ordered log

One Family has a single ordered relay log. Each committed entry has a `u64`
cursor starting at 1 and is either a signed control transition or a signed
encrypted batch. Cursor gaps, duplicate cursors with different bytes, a
control sibling, or rollback below a locally pinned head block sync and are
reported; the client preserves local data. A withheld latest entry may be
undetectable without an independent peer, an accepted limit.

The public authorization state is canonical CBOR map:

| Key | Field |
|---|---|
| 1 | version `1` |
| 2 | Family ID bytes16 |
| 3 | relay ID bytes32 |
| 4 | epoch `u32`, starting at 1 |
| 5 | active devices: array of `[device_id, sign_pk32, agree_pk32, agree_key_version_u32, role]`, sorted by device ID |
| 6 | pending enrollments: array of `[invitation_id, device_id, sign_pk32, agree_pk32, key_version, role, claim_hash32, challenge_id_or_null, proof_hash32-or-null]`, sorted by invitation ID |
| 7 | invitations: array of `[invitation_id, issuer_device_id, invite_sign_pk32, role, issue_transition_id, status]`, sorted by invitation ID |

Roles are `1=member`, `2=manager`; pending devices do not count as managers.
Invitation status is `1=unused`, `2=claimed`, `3=canceled`; expiry is derived
from the relay-signed commit time of the issue transition plus exactly
`604800000` ms. At
least one active manager is required. An active member can read/write data
and issue a delegated grant only for an already pending invitation at its
fixed role; it cannot issue invitations or change other membership. An
active manager may issue/cancel invitations, change roles, remove active or
pending devices, and rotate keys. Every device is scoped to this Family; no
global account or display label occurs in public authority state.

`state_hash = H("auth-state", canonical-CBOR-state)`. Clients derive the
state by applying each transition, then compare that hash to the declared
result. They do not accept a relay-supplied state snapshot without replay or
a separately verified snapshot protocol (none in v1). The plaintext control
chain exposes only opaque IDs, public keys, roles, epoch, commitments, object
hashes, and timing; names and record values stay encrypted.

An unsigned control transition is canonical CBOR map:

| Key | Field |
|---|---|
| 1 | version `1` |
| 2 | Family ID bytes16 |
| 3 | relay ID bytes32 |
| 4 | previous signed control head hash bytes32, all zero on genesis |
| 5 | transition ID bytes16 |
| 6 | kind code from table below |
| 7 | kind-specific public delta map, with exact fields below |
| 8 | resulting `state_hash` bytes32 |
| 9 | epoch `u32` after transition |
| 10 | sorted object manifest entries `[kind_u16, object_id_bytes16, H("object", bytes), length_u32]` |
| 11 | transition core hash bytes32 |

All sorting uses unsigned lexicographic comparison of raw ID bytes, never
signed platform-byte comparison. Signatures are an array
`[signer_device_or_invite_id_bytes16, signature64]` sorted by signer ID;
manifest entries sort by numeric kind then unsigned object ID bytes.
Duplicate primary sort keys are invalid, even when their contents match.
The signed message is `H("control-transition", unsigned_transition_bytes)`.
The relay adds commit receipt body `[family_id, relay_id, transition_id,
cursor_u64, committed_ms_i64, H("control-signed", CBOR([unsigned_transition,
signatures]))]` and signs `H("control-receipt", receipt_body)`. The committed
control object is canonical CBOR `{1: unsigned_transition, 2: signatures,
3: receipt_body, 4: relay_signature64}`; `head_hash = H("control-head",
committed_object_bytes)`. Clients verify the receipt signature, strictly
increasing cursor, nondecreasing relay commit time, and the signed-object
hash before advancing their pinned head. Honest-relay expiration uses that
relay clock. A malicious relay may future-date the issue receipt and extend
bearer-link authority beyond seven real days; this is an accepted
authorization limit of relying on the relay's clock, not merely an
availability limit. Clients show the signed expiry time but cannot
independently prove real elapsed time from this chain.
The previous hash makes one chain. Each client stores its latest accepted
head durably before acting on newer shared state. Genesis is signed by its
initial manager key and commits to that device, epoch 1, relay identity, and
epoch-key commitment. A joining client pins the genesis hash from its link.

Control kind codes and required authorization:

| Code | Kind | Who signs and effect |
|---|---|---|
| 1 | genesis | initial manager; activate complete staged local history |
| 2 | invite_issue | active manager; add unused one-device invitation |
| 3 | invite_cancel | active manager; invalidate unused invitation |
| 4 | invite_claim | invitation key **and** recipient device; consume invite into keyless pending device |
| 5 | key_proof | pending device; record challenge proof, no data key or authority |
| 6 | admit_grant | current active key holder; admit proved pending device at fixed role **with** grant atomically |
| 7 | role_change | active manager; change one active role, keep an active manager |
| 8 | remove_active | active manager; remove one active device and rotate epoch |
| 9 | remove_pending | active manager; remove a keyless pending device, no rotation |
| 10 | grant_repair | active key holder; replace a faulty grant for an already admitted device at current epoch |
| 11 | holder_challenge | active key holder; publish a verifier-bound challenge for one pending device |

The deterministic state effects are: genesis creates epoch 1 with the one
manager; issue inserts an unused invitation; cancel marks an unused one
canceled; claim marks an unused one claimed and inserts one pending row;
challenge replaces that row's challenge ID and clears its proof; proof sets
`proof_hash = H("proof", CBOR([challenge_id, proof_signature64]))`;
admission removes the pending row and inserts its unchanged device keys/role
as active; role change replaces the one role and, on manager-to-member
demotion, marks **all** that issuer's unused invitations terminal canceled;
active removal deletes the target active row, marks all its unused
invitations terminal canceled, and increments epoch exactly once; pending removal deletes
the target pending row; rotation also clears the challenge ID and proof hash
in every still-pending row because their verifier objects bind the old
epoch; repair leaves auth state unchanged. All other rows
stay byte-identical. A later promotion back to manager does not revive an
invitation canceled by authority loss. A device, invitation, transition, grant, challenge, or
batch ID once seen in this Family's verified chain is never reused for a
different object, even after removal. The relay indexes these forever and
clients verify against their history. Unknown kinds reject before mutation.

The public delta contains only the keys specified for its kind:

| Kind | Exact delta map |
|---|---|
| genesis | `{1: initial_device_tuple, 2: epoch_key_commitment32, 3: promotion_manifest_hash32}` |
| invite_issue | `{1: invitation_id, 2: issuer_id, 3: invite_sign_pk32, 4: fixed_role}` |
| invite_cancel | `{1: invitation_id}` |
| invite_claim | `{1: invitation_id, 2: device_id, 3: sign_pk32, 4: agree_pk32, 5: key_version_u32, 6: enrollment_nonce32, 7: claim_hash32}` |
| key_proof | `{1: invitation_id, 2: device_id, 3: challenge_hash32, 4: proof_signature64}` |
| holder_challenge | `{1: invitation_id, 2: device_id, 3: challenge_id, 4: challenge_hash32}` |
| admit_grant | `{1: invitation_id, 2: device_id, 3: fixed_role, 4: epoch_key_commitment32}` |
| role_change | `{1: device_id, 2: old_role, 3: new_role}` |
| remove_active | `{1: device_id, 2: old_role, 3: new_epoch_key_commitment32}` |
| remove_pending | `{1: invitation_id, 2: device_id}` |
| grant_repair | `{1: device_id, 2: admission_transition_id, 3: epoch_key_commitment32}` |

All IDs and key lengths use the definitions above; `initial_device_tuple`
has the same five fields as an active-device row. The state transition is
exactly the table effect, with removal deleting the addressed row and claim
marking its invitation claimed. A claim remains pending if its issuer later
loses authority; an unused invitation becomes invalid at that event without
needing a new status code. `transition_core = CBOR([version, family_id,
relay_id, prior_head, transition_id, kind, exact_delta, resulting_state_hash,
next_epoch])` and `core_hash = H("transition-core", transition_core)`.
The client recomputes and compares key 11. A commitment for each epoch is
recorded in the genesis/rotation delta and cannot be changed by a repair.

`invite_claim`, `holder_challenge`, and `key_proof` are public control
transitions: `invite_claim` and `key_proof` have an empty object manifest,
while `holder_challenge` contains the two challenge objects specified below.
They do **not** require a membership ciphertext or a key holder signature
for a keyless recipient's claim/proof. The public auth state is sufficient
to represent keyless pending progress. Every other non-genesis transition
has an encrypted membership object repeating CBOR map `{1: transition_id,
2: prior_head_hash, 3: resulting_state_hash, 4: next_epoch, 5: exact_delta}`.
It is encrypted under the resulting epoch key for a rotation and otherwise
the current key, with associated data `H("membership-aad", core_hash)`.
The manager or grantor signs a manifest containing its object hash. After
decryption, clients compare every repeated field byte-for-byte to the public
transition; disagreement quarantines the Family and is never projected as a
different UI membership event. A removed device cannot decrypt the new
object but verifies its public removal proof and prior chain.

An honest relay stages manifest objects, verifies sizes and hashes, checks
signatures/roles, enforces the state transition and last-manager invariant,
then commits *all* objects, the transition, resulting state and next cursor
in one durable compare-and-swap against the expected prior head. A failed
CAS exposes no partial authority change. A signed transition ID is unique;
retrying identical bytes returns the first result. Same ID with different
bytes is rejected. A client confirms an access change only after retrieving
and verifying the committed signed transition extending its pinned chain.

## Invitation, proof, and admission

A manager first commits `invite_issue` before displaying a link. The link's
URL fragment contains a bootstrap descriptor with exact relay origin and
relay public key, Family ID, genesis hash, invitation ID, fixed role,
invitation signing-key seed, and hash of the manager-signed issue transition.
The descriptor is canonical CBOR array `[1, relay_origin_text,
relay_sign_pk32, family_id16, genesis_head32, invitation_id16,
fixed_role_u8, invitation_sign_seed32, issue_signed_hash32]`.
`issue_signed_hash = H("control-signed", CBOR([issue_unsigned_transition,
issue_signatures]))`. The fragment is exactly
`#bt-invite=v1.` followed by unpadded base64url of the descriptor bytes;
reject extra fragment keys, padding, noncanonical base64url/CBOR, or a
descriptor over 512 decoded bytes. Fetch the issue transition and receipt,
verify its hash, genesis ancestry, relay pin, role and invitation public key
derived from the seed before claiming. HTTPS URL path/query are only app
routing; they are not trusted bootstrap fields.
The relay never receives the seed. The link contains no Family epoch key,
keyring, or envelope decryptable into one. Anyone with an unused valid link
can claim it first; a preview never contacts the claim endpoint. The first
fresh installation durably creates its own Family-scoped signing/agreement
keys and random enrollment nonce before sending a claim.

For `invite_claim`, the invitation and recipient keys both sign the ordinary
control-transition digest. The delta's `claim_hash` is
`H("claim", CBOR([family_id, relay_id, invitation_id, role, device_id,
sign_pk, agree_pk, key_version, enrollment_nonce, current_head]))`.
The client resolves `created_ms`
from the verified issue receipt, never from an unauthenticated relay field.
The honest relay checks the exact issued role/key, current unused status,
and issuer authority when claiming. In the same CAS that appends the claim,
it assigns the claim receipt's signed `committed_ms` and requires
`claim_committed_ms < issue_committed_ms + 604800000`; a request received
earlier but committed at or after expiry is rejected. Clients verify this
inequality from both relay-signed receipts before accepting the pending
claim. The committed claim
consumes the invitation; only that device's signing key can query/resume it.
A failed request before commit does not consume it. A lost response is
resolved by the stable enrollment nonce and signed device authentication;
the link seed alone cannot take over the committed claim. When unrelated
control changes make `current_head` stale, the same device/nonce may sign a
new attempt after fetching the head, but two claims cannot both commit.

After claim, an active holder generates a random 32-byte secret and random
challenge ID. `context_bytes` is canonical CBOR `[family_id, relay_id,
invitation_id, pending_device_id, claim_hash, challenge_id,
pending_agree_pk, pending_key_version, current_head]`. The holder HPKE-seals
`CBOR([1, secret32])` to the pending agreement key using
`info=H("challenge-info", context_bytes)` and AAD equal to `context_bytes`. It also
encrypts the same plaintext under the current epoch key with a fresh
XChaCha nonce and `AAD=H("challenge-verifier-aad", context_bytes)`. The signed
`holder_challenge` manifest includes both complete objects, whose exact
schemas are below. Its delta includes `challenge_hash = H("challenge",
CBOR([context_bytes_bstr, secret32]))`. The relay receives neither the secret nor any
Family key. Every active holder can decrypt the verifier object and check
the hash; only the pending agreement key can open the addressed HPKE object.

The pending device decrypts the HPKE object and signs
`H("key-proof", CBOR([context_bytes_bstr, secret32]))` with its pending signing key.
The `key_proof` transition carries that signature and challenge hash in its
delta and is signed by the pending key over the control-transition digest.
`current_head` inside `context_bytes` is the verified control head immediately
before `holder_challenge`; it stays fixed when unrelated transitions follow.
`key_proof` is admissible only when its invitation/device names the same
pending row, its challenge ID is that row's latest committed challenge,
the challenge's epoch equals the current epoch, and its public hash matches
that transition. Unrelated intervening transitions do not invalidate it.
A rotation clears pending challenges and proofs, requiring a fresh challenge.
The relay checks those public conditions and signer; the **admission signer**
must independently open the verifier and check both hashes and the pending
proof signature before signing admission. An invalid or missing verifier,
reused challenge ID, wrong pending device/key version, or a stale challenge
is rejected. A holder may publish a fresh challenge after an unrelated head advance; the
pending device proves the latest committed challenge. A client that cannot
prove both keys stays pending; a manager may remove it and reissue an invite.

Any active key holder may then prepare the current epoch key grant. Define
`admission_context = CBOR([core_hash, invitation_id, recipient_device_id,
fixed_role, recipient_agree_pk, key_version, current_epoch,
current_epoch_key_commitment])`. HPKE `info` is
`H("grant-info", admission_context)`, AAD is the canonical context, and
plaintext is CBOR `[version=1, current_epoch, current_epoch_key]`. The grant
object contains suite IDs, recipient ID/key version, HPKE `enc`, ciphertext,
and core hash; its hash is in the signed object manifest. A currently active
key holder signs `admit_grant`, which one CAS commits with the grant and
encrypted membership object. It rechecks proof, fixed role, current epoch,
recipient pending state, sender current access, and object completeness.
**No Family key grant exists for the pending device before this commit.**

Admission gives the device its role before it has downloaded the grant. A
modified admitted client can exercise its role immediately; the normal app
waits to show ready until it verifies the chain, decrypts the addressed
grant, checks the epoch commitment/keyring, and loads the declared history.
That final local acknowledgement is delivery progress, not a second
authorization transaction. If an incorrect grant was committed, any
currently active key holder, including the original grantor, may issue
`grant_repair` bound to the same admission and
current epoch; the app stays not-ready. A manager may remove the admitted
device with rotation. Removing a keyless pending device before admission
requires no rotation; removal after admission cannot retract old history.
An issuer's later removal does not cancel a committed pending claim.

Pending enrollment, challenge, proof, admission, and delivery attempts are
durable and idempotent. Empty wake pushes and normal background sync advance
them when each device separately comes online. There is no simultaneous
online requirement or guaranteed completion time; an unavailable wake leaves
an honest visible pending stage. No second manual manager approval occurs.

## Epoch rotation and history keys

For an active removal, a manager generates a fresh random epoch key. Define
`key_commitment = H("epoch-key", CBOR([family_id, new_epoch, new_key]))`.
The rotation context is canonical CBOR `[core_hash, family_id, relay_id,
prior_head, transition_id, next_state_hash, new_epoch, key_commitment,
exact_delta]`. For every remaining active
device, create an HPKE grant addressed to its current agreement-key version.
Use `info = H("rotation-grant-info", CBOR([core_hash, recipient_device_id,
key_version]))` and AAD equal to the rotation context. The plaintext is CBOR
`[1, new_epoch, new_key]`. Never grant to the removed or still-pending device.

The new-key keyring is canonical CBOR `[1, [[epoch, key32], ...]]`, containing
all earlier numbered epoch keys in ascending order. Encrypt it with the new
key under XChaCha20-Poly1305, random nonce, and AAD `H("keyring-aad",
core_hash)`. Its bytes, nonce, and every recipient grant are in the signed
manifest. Clients verify the entire manifest, open only their addressed
grant, compare the new key commitment, open the keyring, and check **every**
earlier key against commitments in their pinned chain. A missing, substituted,
or extra key blocks activation of that epoch. Because the core hash is formed
before ciphertext hashes, no circular signature/hash dependency exists.

The honest relay atomically commits rotation, grants, keyring, membership
object, state, and cutover cursor. It rejects every later old-epoch batch and
batch from the removed device. A batch already committed before cutover
remains history. A remaining offline device verifies the new head/grant,
checks its pending operation IDs against accepted results, and rebatches only
uncommitted operations with unchanged operation bytes and fresh batch IDs,
nonces, and admissible device sequences. A removed device keeps its local
outbox but cannot upload it; D7 copies unresolved local work privately after
verified removal. A malicious relay hiding the change from a stale writer
can still obtain old-key ciphertext readable by the removed device.

## Signed encrypted batches and receipts

Batch plaintext is canonical CBOR array of at most 256 byte strings, each
containing one canonical [operation](records-v1.md#operation-format). Maximum
plaintext is 256 KiB. The signed header is canonical CBOR map:

| Key | Field |
|---|---|
| 1 | protocol version `[1, 0]` |
| 2 | Family ID bytes16 |
| 3 | relay ID bytes32 |
| 4 | authoring control head hash bytes32 |
| 5 | epoch `u32` |
| 6 | batch ID bytes16 |
| 7 | author device ID bytes16 |
| 8 | next accepted device sequence `u64`, starting at 1 |
| 9 | random XChaCha nonce bytes24 |
| 10 | plaintext byte length `u32` |

AAD is `H("batch-aad", header_bytes)`. Ciphertext includes its Poly1305
tag. The device signs `H("batch-envelope", CBOR([header,
H("batch-ciphertext", ciphertext)]))`. The transmitted object is canonical
CBOR `{1:header, 2:ciphertext, 3:signature64}`. The relay checks signature
against the active author credential, exact object hash/limits, Family and
relay IDs, epoch equality at the atomic commit position, authoring head
ancestry, write role, and next sequence. A prior unrelated invitation does
not force rebatching if epoch and authorization remain valid. The relay
never decrypts. Clients verify the same conditions at the entry's global
cursor, decrypt, and require every operation's Family and author to match.
For receipts and idempotency, `object_hash = H("object",
batch_envelope_bytes)`; it is not a hash of plaintext or ciphertext alone.

Only an **accepted** batch consumes a device sequence. A rejection does not.
The relay durably indexes `(family_id, batch_id)` to the exact bytes and first
result, and `(family_id, device_id, accepted_sequence)` to one accepted
batch. A byte-identical retry returns its first accepted receipt; same ID or
sequence with different bytes is rejected. Receipt bytes are specified
below.
A client also verifies a claimed acceptance by pulling that exact committed
entry and checking its signed envelope and control chain. Rejection from a
malicious relay is not proof of noninclusion on a hidden fork.

On timeout, query by signed batch ID and retry **identical bytes** while the
result is unknown; never sign different bytes with the same sequence. After
a signed sequence rejection, the client first verifies the competing accepted
entry in its complete ordered prefix through the rejection cursor. It then
archives the rejected envelope, keeps its operation bytes, and stages a fresh
batch at the verified next sequence with a new nonce and ID. A rejection
without that competing entry leaves the outbox uncertain. After
a definite old-epoch rejection and verified new control head, use the next
expected sequence and a new batch ID/nonce while retaining operation IDs.
The local outbox removes an operation only after a verified accepted result
in that Family. Clients dedupe operation IDs across accepted batches and
report same-ID/different-bytes as a Family error. If the relay cannot resolve
an unknown outcome, retain the outbox and display uncertainty. A malicious
relay can hide or fork a receipt; v1 does not claim proof of global
noninclusion.

## Object schemas and authenticated requests

All object maps below are canonical CBOR with exactly the listed positive
integer keys, version `1`, no extensions, and a 1 MiB encoded size ceiling.
Each manifest object kind is fixed: `1=membership`, `2=challenge_hpke`,
`3=challenge_verifier`, `4=epoch_grant`, `5=keyring`,
`6=promotion_manifest`, `7=promotion_chunk`. `object_id` is a distinct
random bytes16 ID except a grant uses its grant ID and a challenge pair uses
two distinct IDs. A manifest entry's length is the byte length of the full
object, and the hash covers those full bytes. Duplicate kind/ID pairs,
unlisted objects, missing listed objects, and a manifest over 16,384 entries
reject before commit. A signer verifies its complete object set before
signing; a reader verifies every manifest hash before using any object.

| Kind | Exact CBOR map |
|---|---|
| membership | `{1:1, 2:nonce24, 3:ciphertext_with_tag}`; plaintext is the five-key membership map above; AAD is `H("membership-aad", core_hash)` |
| challenge_hpke | `{1:1, 2:challenge_id16, 3:recipient_device_id16, 4:key_version_u32, 5:context_bytes, 6:enc32, 7:ciphertext_with_tag}` |
| challenge_verifier | `{1:1, 2:challenge_id16, 3:context_hash32, 4:nonce24, 5:ciphertext_with_tag}`; context hash is `H("challenge-context", context_bytes)` |
| epoch_grant | `{1:1, 2:grant_id16, 3:purpose, 4:recipient_device_id16, 5:key_version_u32, 6:[32,1,3], 7:core_hash32, 8:enc32, 9:ciphertext_with_tag}` |
| keyring | `{1:1, 2:nonce24, 3:ciphertext_with_tag}` |

Grant purpose is `1=admission`, `2=rotation`, `3=repair`. Purpose 1 uses
the admission context above, 2 the rotation context above, and 3
`repair_context = CBOR([core_hash, family_id, relay_id, prior_head,
transition_id, admission_transition_id, recipient_device_id,
recipient_agree_pk, key_version, current_epoch, current_key_commitment])`.
Repair uses `info=H("repair-grant-info", repair_context)`, AAD equal to the
context, and plaintext `[1, current_epoch, current_key]`. Only the device
admitted by the named transition and still active may receive it. An
active holder signs repair; it cannot change role, key version, epoch, or
commitment. A malicious or faulty authorized grantor can still commit an
unusable grant, delaying local readiness. Another holder can repair, or a
manager can remove with rotation. Admission is authority at commit even
when local delivery is not ready.

Challenge HPKE `context_bytes` is the exact canonical context in the join
section. The verifier object encrypts `[1, secret32]` under the current
epoch key with `AAD=H("challenge-verifier-aad", context_bytes)`. A proof
is valid only if both objects, decrypted secret, committed challenge hash,
claim hash, and recipient signature agree. A holder does not rely on the
relay's statement that the challenge was delivered. A verifier encrypted
under a later epoch requires a fresh holder challenge.

Accepted and rejected batch receipts use one exact CBOR body map:
`{1:1, 2:family_id16, 3:relay_id32, 4:batch_id16, 5:object_hash32,
6:accepted_bool, 7:cursor_u64, 8:control_head32,
9:device_sequence_u64, 10:reason_u16_or_null,
11:next_expected_sequence_u64}`. Reasons are `1=stale_epoch,
2=revoked, 3=stale_head, 4=sequence, 5=invalid`; success has reason null.
For success, cursor is the accepted entry's cursor, sequence is its accepted
sequence, and next expected is sequence+1. For rejection, cursor/head are
the relay's current values, sequence is the attempted sequence, and next
expected is unchanged. The wire receipt is `{1:body, 2:relay_signature64}`
with signature over `H("batch-receipt", body_bytes)`. The client treats an
unsigned HTTP error as unknown outcome, not a definite rejection.

### Relay routes and response bytes

All wire request and response bodies below are canonical CBOR with media type
`application/cbor`; maps reject unknown keys. IDs in paths are exactly 32
lowercase hex digits, decimal cursors have no leading zero except zero, and
method text is uppercase ASCII `GET` or `POST`. The server rejects percent
escapes, dot segments, path normalization aliases, extra query parameters,
and paths differing from the signed bytes. An unsigned HTTP error or timeout
is an unknown outcome, never proof that a write did not commit.

Read authentication uses `request_bytes = CBOR([1, family_id, relay_id,
signer_id16, random_request_id16, "GET", exact_path_text,
H("request-body", empty_bytes)])`, with wire CBOR
`{1:request_bytes_bstr, 2:signature64}` signed over
`H("read-request", request_bytes)`. A duplicate request ID with different
bytes is rejected. Before genesis commits, the declared initial manager may
query only the promotion result using the signing key in the reserved
genesis candidate. An invitation key uses its invitation ID as signer ID and
may fetch the public control chain and issue object but no Family data
objects. A pending device may fetch that chain and its addressed
challenge HPKE object; an active device may fetch all committed Family
objects and entries; a removed device may fetch the public control chain,
its own earlier batch-result receipts, and a removal proof. The relay checks
Family and signer binding on every path. These are honest-relay read ACLs;
encryption still protects data if a malicious relay serves extra ciphertext.

| GET path | Exact successful response body |
|---|---|
| `/v1/families/{family_hex}/log?after={cursor}` | page map below, all entry kinds; active device only |
| `/v1/families/{family_hex}/control?after={cursor}` | page map below, control entries only; authenticated invitation/pending/active/removed device |
| `/v1/families/{family_hex}/batches?after={cursor}` | page map below, batch entries only; active device only |
| `/v1/families/{family_hex}/objects/{object_hex}` | `{1:1, 2:kind_u16, 3:object_id16, 4:object_bytes_bstr, 5:committing_transition_id16}` |
| `/v1/families/{family_hex}/batch-results/{batch_hex}` | `{1:1, 2:signed_receipt_bytes_bstr_or_null}`; null is unknown, not noninclusion |
| `/v1/families/{family_hex}/control-results/{transition_hex}` | `{1:1, 2:committed_control_bytes_bstr_or_null}`; null is unknown |
| `/v1/families/{family_hex}/invites/{invitation_hex}` | `{1:1, 2:committed_issue_control_bytes_bstr}`; current validity is derived by replaying the control chain |
| `/v1/families/{family_hex}/promotions/{promotion_hex}` | `{1:1, 2:committed_genesis_bytes_bstr_or_null}`; null is unknown |

A page is CBOR `{1:1, 2:family_id16, 3:requested_after_cursor_u64,
4:entries, 5:next_after_cursor_u64, 6:has_more_bool}`. Each entry is
`[global_cursor_u64, kind_u8, committed_bytes_bstr]`, with kind `1=control`,
`2=batch`. Entries strictly increase and contain only cursors greater than
`requested_after`; a full-log page has no global gaps. Filtered control or
batch pages may skip cursors occupied by the other kind. At most 256 entries
and 4 MiB of encoded entries are returned. `next_after` is the last returned
cursor, or the requested cursor when empty; `has_more` states that another
matching entry exists. Clients advance only after verifying each included
signed entry. An unlisted latest entry may still be hidden by a malicious
relay, as already accepted.

An invitation or pending device verifies the signed, parent-linked control
ancestry and each control's global cursor from this filtered feed. It may
advance its pending control state across intervening batch cursors without
claiming that those batches or their data are verified. Its local Family is
not data-ready at admission. Once active, the device fetches the full log
from its pinned genesis through admission, verifies every contiguous entry
and required object and batch receipt, then establishes data readiness.
The relay does not expose skipped batch bytes through pending read access.

Normal object staging is `POST
/v1/families/{family_hex}/objects/{object_hex}` with body CBOR
`{1:1, 2:unsigned_transition, 3:sorted_signatures,
4:manifest_kind_u16, 5:object_id16, 6:object_bytes_bstr}`. The first two
fields are the **signed candidate** lacking a relay commit receipt. Its
manifest must contain exactly the named kind/ID, byte length, and
`H("object", object_bytes)`. Staging authorization depends on kind:
for a **genesis** candidate, the relay requires an absent Family, an
all-zero prior head, a valid signature from the initial-manager public key
declared in its genesis delta, and an uncommitted reservation keyed by
`(family_id, promotion_id, H("genesis-reservation", candidate_bytes))`,
where `candidate_bytes` is canonical CBOR `{1:unsigned_transition,
2:sorted_signatures}`.
The first valid stage creates that reservation; another candidate for the
same Family ID is rejected. This reservation conveys no active membership,
read access, or joinable Family. For **every non-genesis** candidate, the
relay verifies candidate signatures, Family/relay/prior-head binding,
current signer authority at that head, and its public delta before staging.
An object stage under either rule must match the candidate's manifest.
A duplicate `(family_id, object_id)` with
identical bytes is idempotent; different bytes are rejected. The response is
`{1:1, 2:object_hash32, 3:true}`. Staging confers no authority or read
access. Uncommitted staged bytes are not exposed through GET. The relay may
remove uncommitted bytes after 24 hours, but permanently retains the
ID-to-hash reservation; an identical retry restages missing bytes. A failed
CAS never exposes partial objects or changes public state.

`POST /v1/families/{family_hex}/control` carries candidate CBOR
`{1:unsigned_transition, 2:sorted_signatures}`. After checking every
manifest object is staged, the relay revalidates the applicable genesis or
current-head authorization rule and CAS, then atomically appends the transition,
objects, state, receipt, and cursor. Success body is
`{1:1, 2:committed_control_bytes_bstr}`. `POST
/v1/families/{family_hex}/batches` carries the signed batch envelope bytes;
success or definite rejection body is `{1:1, 2:signed_batch_receipt_bytes}`.
A retry of exact bytes returns the first result. Clients confirm success by
fetching and verifying the matching committed entry, not by HTTP status
alone. Promotion stages its signed genesis candidate's manifest/chunks
through the same object route; the promotion-result GET above resolves a
lost genesis response. All POST paths have no query string.

## Verified removal and private copy

A removed device retains read-only access to the opaque signed control chain
through the authenticated `control?after={pinned_cursor}` route above, using
its old device signature and verifying that the returned chain extends its
locally pinned head. It cannot fetch new ciphertext/grants or append. A verified
removal proof is a chain extending its pinned head, with a valid authorized
`remove_active` transition targeting its exact device ID and a checked
resulting state/epoch. An unsigned access-denied response, timeout, or a
chain that does not extend the pinned head is **not** proof of removal and
never triggers automatic private copying. A relay that withholds the proof
can delay the notification; users may deliberately copy locally held data
at any time.

The automatic private copy idempotency key is `(source_family_id,
removed_device_id, removal_transition_id)`. In one local transaction the
client reserves a fresh Family/device identity, preserves child/activity
record IDs under the new Family, remaps the Family metadata record ID and
all operation IDs and authors, includes pending work and the triggering
action ID, builds a new log/projection with fresh keys, and marks the source
outbox archival. Commit destination identity, operation IDs, deduped action
delivery IDs, and source link together; publish the destination only after
commit. Restart or repeated widget/watch delivery resumes the same copy.
Fresh operations receive a fresh local append order and HLC metadata;
original operation IDs are retained only as local provenance. The source
history stays locally readable/exportable and never uploads under
the revoked credential. A deliberate copy by a still-authorized member
after sole-manager loss uses the same transaction but a user-chosen stable
copy ID; it does not remove any other Family member or delete relay data.
The copy contains only history locally held at its snapshot point and marks
known sync gaps rather than asserting completeness.

## Promotion, isolation, and trust limits

Local creation needs no relay. The Family and record/operation IDs remain
stable when sharing is enabled. In one local transaction, record promotion
ID, watermark, initial manager/epoch commitment, and a flat manifest of
encrypted history chunks through that watermark. Each chunk ends on an
operation boundary and is at most 256 KiB plaintext; the manifest lists
ordered chunk hashes, covered local append indexes, and count. The manifest
object is canonical CBOR `{1:1, 2:family_id16, 3:relay_id32,
4:promotion_id16, 5:watermark_u64, 6:ordered_chunks}`. Each chunk row is
`[index_u32, object_id16, object_hash32, encoded_length_u32,
first_local_index_u64, last_local_index_u64]`; rows start at index zero,
have contiguous local index ranges from one through watermark, and a
watermark of zero has no chunks. A chunk object is CBOR `{1:1,
2:[family_id, relay_id, promotion_id, chunk_index_u32, epoch=1,
nonce24], 3:ciphertext_with_tag}`. Its plaintext is a canonical CBOR array
of canonical operation byte strings in local append order; AAD is
`H("promotion-aad", header_bytes)`. No chunk has more than 256 operations
or 256 KiB plaintext. The genesis object manifest includes this manifest
and every chunk, and its public delta binds the manifest object hash.
The relay
stages by promotion ID and exposes no joinable Family. After all objects
are present, one CAS commits genesis and history at cursor 1. A lost response
is queried by promotion ID and manifest hash. Until that result verifies,
the UI says sharing pending while local writes continue above the watermark.
After activation, post-watermark operations use ordinary batches. A joiner
checks all declared chunks before reporting history loaded. No partial
manifest is accepted.

Every durable key, outbox item, record, projection row, cursor, grant,
control head, and retry identity is scoped by Family ID. Local databases use
composite `(family_id, object_id)` primary/foreign keys and typed Family
handles; they reject cross-Family references before encryption or projection
and never search a global key fallback. Signatures and AEAD contexts bind
Family and relay IDs. A restored or forked Family gets new Family/device
identities and keys, preserves child/activity record IDs under the new Family
scope, and remaps operation IDs. Other Families
continue independently if one is revoked or blocked. Network/push metadata
can still correlate a device's Families; isolation is not anonymity.

An honest relay provides one order, single-use claims, CAS races, and signed
receipts. A malicious relay can show separate valid control branches, hide
the latest batch/removal, or refuse service. Pinning detects rollback or a
sibling **once seen** but not a permanently hidden fork. Thus two branches
can each consume one invitation or choose a different first removal. V1
accepts this limit and will state it in the UI/security documentation; an
independent witness or peer gossip is deferred because it would change
availability and deployment assumptions. Relay signatures do not cure
equivocation by the relay that owns that key.

Standards: [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949),
[RFC 9180](https://www.rfc-editor.org/rfc/rfc9180),
[RFC 8032](https://www.rfc-editor.org/rfc/rfc8032).
