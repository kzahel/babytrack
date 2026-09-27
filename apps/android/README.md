# Android development build

The debug app tracks local Families and runs the first two-device sharing
handoff on a development relay: promotion, invitation, claim, challenge,
proof, grant, and recipient history verification. Both devices can edit and
sync; each can save a shared backup or make an independent private copy.
While the app is in the foreground it polls the relay every 30 seconds. A
persisted Android job also requests network sync when the OS allows it.
The manager can remove the first recipient. After verifying a signed
removal, that device stops shared writes and automatically makes a private
Family copy when it has pending changes. The release manifest does not allow
cleartext HTTP; sharing controls remain in the debug UI while broader
membership and the M0 security gate are open.

To exercise the current relay slice on an emulator:

1. Create a private 32-byte relay seed file and start `babytrack-server` with
   a persistent SQLite path and `127.0.0.1:8787` bind address. The server
   prints its **public** relay key; keep the seed file private.
2. Run `adb reverse tcp:8787 tcp:8787`, then install the debug APK built by
   `apps/android/gradlew :app:assembleDebug`.
3. Open **Sharing controls**. In **Sharing setup preview**, enter
   `http://localhost:8787` and the printed 64-digit relay public key.
   Use **Set up or retry sharing**, then
   **Create or retry invitation**. Exact prepared bytes survive process
   restart and uncertain POST responses; a confirmed invite yields a
   one-device fragment.
4. On a receiving debug app, choose **Join a Family** and paste that fragment
   in **Join preview**. The app
   signs an authenticated read, verifies the invitation-linked public
   controls in Rust, and submits the exact durable claim. It reports waiting
   for a key holder; this stage does not provide shared data yet. Retrying
   after restart resubmits the saved claim without rereading through the
   invitation credential, whose read access closes at claim commit.
5. On the holder's debug screen, use **Respond to pending device**. The app
   verifies the claim, commits a challenge addressed to the recipient, and
   reports that it is waiting for the recipient's proof. Retrying after
   restart reuses the exact saved challenge.
6. On the recipient's debug screen, use **Prove device key**. Rust verifies
   and opens only the challenge addressed to this device, then commits a
   durable proof. Retrying after restart uses the exact saved proof.
7. On the holder's debug screen, use **Grant device access**. Rust verifies
   the proof against the saved challenge and commits the encrypted grant.
   The recipient remains pending until it loads and verifies the full history.
8. On the recipient's debug screen, use **Load shared history**. A bounded
   sync pass verifies signed control and batch history, fetches required
   encrypted objects, and opens the grant. It reports the verified cursor or
   readiness; repeat after a pending result. The join preview then shows
   verified shared children and entries. Adding a child or wet diaper saves
   offline in the Rust shared outbox; **Sync shared Family** uploads the next
   signed batches and verifies the relay log. The manager can use the same
   button for its Family. A newly shared manager Family uses the shared Rust
   view and edit path; the local-only API rejects it. A lost upload response
   keeps exact batch bytes for retry. A signed rejection is checked against
   verified history before the outbox may be resealed; other rejections keep
   the local operation pending.
9. After app restart, select the joined Family in the join preview to resume
   proof, history loading, or manual sync without pasting the invitation
   again. The manager's relay origin is saved after sharing is confirmed.
   If both apps run at different times, each foreground or scheduled pass
   advances the next verified protocol step. A failed pass leaves local work
   saved and shows delayed or blocked status until resolved.
10. The tracking screen can save a growth entry with whole grams and
    millimetres, a Celsius decimal entry, and a medication name with entered
    dose amount/unit. The same Rust operation path is used for local and
    shared Families; the two-emulator test verifies
    the recipient's entries on the manager's device. The manager can remove
    that first recipient; pending edits on the removed device become an
    independent private Family copy.
11. A device granted manager access can create another invitation from its
    joined-Family debug view. Its foreground and scheduled sync passes answer
    the next device's claim and proof. The real-relay instrumentation suite
    checks this with three separate stores and an encrypted edit from the
    admitted manager. This remains a debug flow; role changes and invitation
    cancellation are still pending.

The on-device integration test calls the same Keystore and transport adapter
through a real relay. With an emulator running, build both APKs with
`apps/android/gradlew :app:assembleDebug :app:assembleDebugAndroidTest`, then
run `bash scripts/check_android_relay_emulator.sh`. The script starts a
disposable relay. To use an already running relay, set
`BABYTRACK_RELAY_PUBLIC_KEY` to its public key. For a direct instrumentation
run after `adb reverse` and both APK installs, use:

```sh
adb shell am instrument -w -e relayPublicKey PUBLIC_KEY_HEX \
  org.babytrack.app.test/androidx.test.runner.AndroidJUnitRunner
```

The separate `python3 scripts/check_android_ui_smoke.py` check installs the
debug APK, clears its data on an emulator, then uses the visible UI to create
a Family and child, log a wet diaper, and verify both after app restart.
Set `ANDROID_SERIAL` when multiple emulators are attached. CI runs it after
the relay instrumentation suite.

The test creates its own Family, promotes a child record, commits an invite,
reopens its store, and retries both requests. A separate recipient store then
claims the invitation and retries after restart with the same device identity
and candidate bytes. The holder then commits and retries the challenge; the
recipient fetches its addressed object and commits and retries its proof. The
holder commits and retries admission. The recipient loads the full history,
including the manager's child, then reopens the store and verifies readiness
again. All three handoff steps are retried after later controls have
committed. The recipient then saves a child and diaper offline, uploads them,
and the manager pulls them. The manager saves another child, uploads it, and
the recipient pulls it. The test reopens the recipient store and resumes by
Family identity without the fragment. It also drops one accepted upload
response and recovers through the signed log. A second test alternates
manager and recipient foreground passes to finish the join without another
manual holder action. A third test forces the scheduled background job and
checks that a saved manager edit uploads without the UI. Android CI now runs
the suite in an emulator against a disposable relay; its remote result is
unverified until an Actions run is inspected.
