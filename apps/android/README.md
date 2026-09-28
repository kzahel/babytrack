# Android development build

The debug app logs a Family's children, feeds, diapers, sleep, pumping,
growth in g/kg/lb/oz and mm/cm/in, medication, temperature, solids, and
notes through the shared Rust core. Local tracking works without a relay or
account. A Family can later be shared through the development relay, and an
invited device gets the same
tracker after its grant and history are verified. The app saves readable or
password-protected backups and restores either into a new local Family.
The timeline can attach, edit, or clear a note on an existing activity.

## Try sharing on two devices

1. Create a private 32-byte relay seed file. Start `babytrack-server` with a
   persistent SQLite path, that seed file, and a reachable bind address. It
   prints a 64-digit **public** relay key; keep the seed private. For an
   emulator connected to a laptop relay at `127.0.0.1:8787`, run
   `adb reverse tcp:8787 tcp:8787` and use `http://localhost:8787` in the app.
   For physical phones, use an origin reachable from both phones.
2. Build and install the debug APK with
   `apps/android/gradlew :app:assembleDebug`. On the first phone, create a
   Family and child. Open **Sharing controls**, enter the exact relay origin
   and printed public key, and tap **Set up or retry sharing**. The app shows
   confirmation only after verifying the signed genesis.
3. Choose **Member** or **Manager**, then tap **Create or retry invitation**.
   Share the one-use link with the second phone using **Share invitation**.
   The recipient can open the link or paste its fragment under **Join a
   Family**. Opening a link prefills the form; **Start or retry join** is the
   explicit claim action.
4. The holder and recipient apps advance challenge, proof, grant, and history
   loading on foreground and scheduled sync passes. They do not need to stay
   open together. The join card shows the current stage and labels saved
   recipient Families as joining or ready after restart. A confirmed claim
   clears the consumed launch link, so reopening the activity shows the saved
   join attempt; a verified ready
   Family appears in the normal Family switcher. Android runs foreground
   passes every 30 seconds and requests scheduled network work when the OS
   permits, so a suspended app may progress later.
5. Log on either phone, including while offline. A saved shared edit remains
   local until a signed relay result confirms it. The access section shows
   verified devices and pending writes. A manager can invite another device,
   label devices on this phone, change admitted roles, cancel an unused link,
   or remove an admitted device. Removal rotates the Family key. After
   verifying removal, a device stops shared writes and preserves pending
   edits in a private Family copy. When background sync creates that copy,
   the app identifies and opens its exact destination on the next foreground
   visit. Removed devices also see an access-ended
   card that opens or creates a private copy of locally held history.

The relay and public key are manual debug setup. Debug builds allow local
cleartext HTTP; the release manifest does not. Physical two-phone use and
the end-of-M0 security gate remain open, so this is a development flow.

## Automated checks

The real-relay Android instrumentation suite uses separate device stores and
a disposable relay. With an emulator running, build both APKs and run:

```sh
apps/android/gradlew :app:assembleDebug :app:assembleDebugAndroidTest
bash scripts/check_android_relay_emulator.sh
```

Set `ANDROID_SERIAL` when multiple emulators are attached. The runner starts
its own relay; set `BABYTRACK_RELAY_PUBLIC_KEY` to use an already running
relay. The suite covers delayed joining, encrypted edits, later invitations,
role changes, two rotations, removal, offline work, and private copies.

The command-driven local UI check creates and edits records through visible
controls; the document-picker recovery check reinstalls the app and restores
readable and protected files. Run them with:

```sh
python3 scripts/check_android_ui_smoke.py
python3 scripts/check_android_ui_smoke.py --quick
python3 scripts/check_android_recovery_ui.py
```

Push and PR CI run the quick path through Family setup, diaper logging,
correction, deletion, and restart, plus the full real-relay instrumentation
suite and file recovery. The complete local UI walkthrough runs on the daily
scheduled and manual workflow, and remains available with no flag. A physical
phone is still needed for one-handed use, large text, night display,
actual reboot/widget placement, and two-caregiver network behavior.
