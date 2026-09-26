# Sync and encryption

Status: proposed design, September 2026. Must be settled before M0 code.
Owns the operation log, merge rules, keys, invites, removal, recovery, server
API, protocol versioning, and threat model. The event payloads themselves are
owned by [event-model.md](event-model.md).

## Goals

- Every caregiver device holds the whole family history and works offline.
- Concurrent edits from any number of devices converge without loss.
- The server stores and relays ciphertext only. It cannot read events,
  children's names, or anything else a family logs.
- A removed caregiver cannot read anything written after removal.
- Losing a phone does not lose the data.
- Clients on different app versions interoperate without dropping data.

## Operation log and merge

Decided.

- The unit of change is an **operation** on one event: `create` (id, type,
  fields), `set` (id, field, value), or `delete` (id). Every operation
  carries a hybrid logical clock (HLC) stamp: wall-clock milliseconds, a
  counter, and the writing device's id as the final tiebreak. HLC stamps give
  every operation a total order that respects causality on each device.
- **Merge** is last-writer-wins per field by HLC. `delete` sets a tombstone
  field, so a later `set` of the tombstone can undelete. A `set` on an event a
  replica has not seen yet is kept and applied when the `create` arrives.
- Operations are grouped into **batches** for upload. Each batch has a
  client-generated id (so retried uploads are idempotent), the writing
  device's id, and a per-device sequence number inside the ciphertext so
  other members can detect a gap the server has hidden.
- Each client folds the entire log in memory. At about 30 events a day the
  log is roughly 22K events after two years, which folds in milliseconds. No
  snapshots in M0.

## Keys

Decided, except where noted under Open questions.

- **Epoch keys.** Batches are encrypted with a random 256-bit epoch key using
  XChaCha20-Poly1305. The associated data binds the family id, epoch number,
  protocol version, batch id, and device id, so a ciphertext cannot be
  replayed into another family or epoch.
- **Key holders.** Every family member that can receive keys is a key
  holder with an X25519 key agreement keypair and an Ed25519 signing keypair.
  There are three kinds, all handled the same way:
  - a **device**, whose private keys never leave it;
  - a **platform backup**, whose private keys live in iCloud Keychain
    (synchronizable) or Block Store;
  - a **recovery phrase**, whose keypairs are derived from 24 words.
- **Membership** is recorded inside the encrypted log as signed operations:
  a holder joined (with its public keys and a display name) or was removed.
  Clients verify signatures against their own membership view.
- **Grants.** A new epoch key is wrapped for each current holder with HPKE
  (RFC 9180, X25519, HKDF-SHA256, ChaCha20-Poly1305) and stored on the server
  addressed by an opaque holder id.
- **Keyring.** Each rotation also stores the list of all earlier epoch keys
  encrypted under the new one, so a newly joined holder can read the full
  history while a removed one gains nothing.
- **Server auth.** Each epoch derives an auth keypair from the epoch key with
  HKDF. The server stores only its public key and requires requests to be
  signed with it. The server never holds anything that decrypts data, and a
  removed holder loses server access at the next rotation.

This refines the plan's original single family secret. Per-holder keypairs
are needed because a removed device that knows the old shared key could
otherwise read the new key while it is handed to the others.

## Invites, removal, and recovery

- **Invite in person.** The inviting device shows a QR code carrying the
  server URL, family id, current epoch, and epoch key. The new device
  generates its keypairs, appends a signed "joined" operation, and from then
  on receives grants like any other holder.
- **Invite remotely.** The same payload in the URL fragment of a link. The
  link is as sensitive as the key, so the app warns before sharing it.
- **Removal.** Any caregiver can remove a holder. The removing device creates
  epoch e+1, grants it to every remaining holder, posts the keyring and the
  new auth public key, and appends a signed "removed" operation. The server
  then rejects the old auth key.
- **Recovery.** A new device that has the recovery phrase or restores the
  platform backup derives or loads that holder's keypairs, fetches its grant
  for the latest epoch, and joins as a new device. Rotating the phrase or
  backup is a removal of the old holder plus a new join.

## Server API

A relay with no knowledge of event content. Sketch, to be fixed in the M0
spec:

- create a family (returns family id, stores first auth public key);
- append a batch (idempotent by batch id; server assigns a sequence number);
- list batches after a sequence number;
- put and get grants and the keyring for an epoch;
- rotate the epoch (new auth public key; old one stops working);
- a WebSocket for live notification of new batches;
- register a push token and send empty wake pushes on new batches.

The push sender is an interface with APNs and FCM implementations first and
UnifiedPush later (see the plan's service interface requirements). Quotas
limit storage per family and requests per family and IP.

## Encoding and versioning

- Operations and batch payloads use CBOR with integer keys. Only the Rust
  core reads the format, so no cross-language schema tooling is needed.
- Unknown event types and unknown fields are kept verbatim and written back
  unchanged. An old client never drops what a newer one wrote.
- Every batch header carries a protocol version. Minor versions are
  additive. A client that sees a newer major version keeps reading what it
  can, stops writing, and asks the user to update.

## Crypto implementation

Pure-Rust RustCrypto crates (`chacha20poly1305`, `hkdf`, `x25519-dalek`,
`ed25519-dalek`) and an HPKE crate, so the same code runs natively and in
wasm. No custom primitives. Every construction gets known-answer vectors in
`spec/`.

## Threat model

The server, or anyone who compromises it, **can see**: which families exist,
how many holders each has (from grant addressing), when batches are written
and how large they are, epoch rotations, client IP addresses, and push
tokens. Activity timing reveals when a family is awake.

The server **cannot**: read or forge events, names, or membership; add a
holder; or read data after it is written under a key it does not hold.

A malicious server **can**: withhold, delay, or delete batches, or serve a
stale view. Per-device sequence numbers let clients detect hidden gaps, and
local-first storage means deleting server data does not delete a family's
history. A hosted web client can also be served malicious code; see the
plan's web client trust note.

## Validation

- Property tests: random operations delivered in random orders to several
  replicas always converge to identical state.
- Known-answer vectors for every crypto construction and for encoding, run
  from every language binding.
- Rotation tests: a removed holder cannot decrypt batches from later epochs,
  and the server rejects its requests.
- Recovery tests: phrase and platform backup both restore full history on a
  fresh device.
- Sync torture test with a real server and many simulated clients going
  offline and reconnecting.
- Plaintext marker test: known strings written into events never appear in
  the server database or logs.

## Open questions

1. **Rotation race.** A removed holder still has the old auth key until the
   rotation lands and could try to rotate first, excluding everyone else.
   Proposed answer: the server accepts only the first rotation from each
   epoch, and clients that find themselves missing from a grant treat the
   family as compromised and re-create it. Acceptable against an ex-partner
   or former nanny; revisit if the threat model grows.
2. **Remote invites with approval.** A safer remote invite would carry a
   short-lived invite key and require the inviter to approve the new
   device's keys. It needs the inviter online. Decide whether it replaces
   the plain link for the MVP.
3. **Encoding.** Confirm CBOR over protobuf once the core exists. The
   requirement is verbatim preservation of unknown fields.
4. **Web key storage.** The web client keeps its holder keys in IndexedDB.
   Decide whether to require a passphrase to unlock them on shared
   computers.

## Reconsider if

- The log grows large enough that full replay is slow on a low-end phone:
  add encrypted client-generated snapshots.
- Families need to share a subset of data, such as a daycare seeing only
  today's feeds: this needs per-child or per-audience keys and read-only
  server auth.
- Users need roles such as read-only caregivers: symmetric epoch keys cannot
  enforce read-only, so writes would need per-holder signatures checked by
  every client.

## References

Signal (libsignal, Signal-iOS, Signal-Android) for device linking by QR and
per-device keys. Actual Budget for HLC-based sync through a relay. Ente for
recovery key UX. Automerge and Loro as the alternatives this design rejects
for being more general than the data needs.
