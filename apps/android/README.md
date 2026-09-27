# Android development build

The debug app tracks local Families and runs the first two-device sharing
handoff on a development relay: promotion, invitation, claim, challenge,
proof, grant, and recipient history verification. The shared tracking view,
shared upload, automatic sync, and removal are still being implemented. The release
manifest does not allow cleartext HTTP.

To exercise the current relay slice on an emulator:

1. Create a private 32-byte relay seed file and start `babytrack-server` with
   a persistent SQLite path and `127.0.0.1:8787` bind address. The server
   prints its **public** relay key; keep the seed file private.
2. Run `adb reverse tcp:8787 tcp:8787`, then install the debug APK built by
   `apps/android/gradlew :app:assembleDebug`.
3. In **Sharing setup preview**, enter `http://localhost:8787` and the
   printed 64-digit relay public key. Use **Set up or retry sharing**, then
   **Create or retry invitation**. Exact prepared bytes survive process
   restart and uncertain POST responses; a confirmed invite yields a
   one-device fragment.
4. On a receiving debug app, paste that fragment in **Join preview**. The app
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
   offline in the Rust shared outbox; upload is not connected yet.

The on-device integration test calls the same Keystore and transport adapter
through a real relay. With the emulator and `adb reverse` running, build it
with `apps/android/gradlew :app:assembleDebugAndroidTest`, install the test
APK, then run:

```sh
adb shell am instrument -w -e relayPublicKey PUBLIC_KEY_HEX \
  org.babytrack.app.test/androidx.test.runner.AndroidJUnitRunner
```

The test creates its own Family, promotes a child record, commits an invite,
reopens its store, and retries both requests. A separate recipient store then
claims the invitation and retries after restart with the same device identity
and candidate bytes. The holder then commits and retries the challenge; the
recipient fetches its addressed object and commits and retries its proof. The
holder commits and retries admission. The recipient loads the full history,
including the manager's child, then reopens the store and verifies readiness
again. All three handoff steps are retried after later controls have
committed. The
default Android CI compiles this test, while
execution currently uses the local emulator and relay.
