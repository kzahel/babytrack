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
cleartext HTTP; the release manifest does not. The bounded M0 security gate passed; physical two-phone validation remains
open. This is a development flow; see [005](../../docs/tactical/005-m1-android.md)
for current evidence and remaining gates.

## Automated checks

The app supports API 26. Android binding generation uses
`apps/android/uniffi.toml` to select UniFFI's SDK-guarded Android cleaner,
with JNA cleanup below API 34. JVM smoke bindings retain JVM generation.
An API 26 Robolectric regression checks fallback selection and exactly-once
cleanup. Navigation-bar appearance attributes introduced in API 27 live in
the matching version-qualified theme resources. Run the compatibility gate
without a lint baseline:

```sh
apps/android/gradlew :app:testDebugUnitTest :app:lintDebug
```

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

These entry points share ADB/UIAutomator support in
`scripts/android_ui.py`; importing helpers does not execute a scenario.
Commands have bounded subprocess timeouts. Each runner emits stage progress
and keeps available screenshot, recent logcat, and last UI XML diagnostics
under `app/build/outputs/ui-diagnostics/` on failure. CI retains those files;
`BABYTRACK_ANDROID_DIAGNOSTICS_DIR` can override the output root. Diagnostic
failure preserves the original scenario error. Harness regressions run with
`python3 scripts/test_android_ui.py` and in the emulator CI runner.

Push and PR CI run the quick path through Family setup, diaper logging,
correction, deletion, and restart, plus the full real-relay instrumentation
suite and file recovery. The complete local UI walkthrough runs on the daily
scheduled and manual workflow, and remains available with no flag. A physical
phone is still needed for one-handed use, large text, night display,
actual reboot/widget placement, and two-caregiver network behavior.

## Screen development and fixture gallery

Render all existing screens without an emulator:

```sh
bash scripts/render_android_gallery.sh
```

Open `apps/android/app/build/outputs/screen-gallery/index.html` to scroll
through the screen gallery. The default is a 412 × 915 dp phone viewport,
rendered at 2× image scale for sharper full-size PNGs. Each case shows dark
and light modes at 100% text first, followed by 150% accessibility checks. Expand additional scroll positions to see the bottom of long
forms and History; click any image for full size. The gallery also supports
search. To iterate on one screen or family of states:

```sh
bash scripts/render_android_gallery.sh -PgalleryCase=capture-bottle
```

For the optional 360 × 800 dp compact layout stress check:

```sh
bash scripts/render_android_gallery.sh -PgalleryViewport=compact
```

It writes a separate `outputs/screen-gallery-compact/index.html`, preserving
`screen-gallery/`. Combine `-PgalleryViewport=compact` with `-PgalleryCase=…`
for a focused compact render. The manifest and HTML record the selected
viewport, image scale, and text size. Robolectric runs Android on the JVM;
Roborazzi captures the actual Compose screens without booting an emulator.

The filtered command regenerates a filtered gallery (including empty and invalid
bottle drafts). Run the unfiltered command before sharing a full gallery.
The first run downloads renderer dependencies and builds generated bindings;
subsequent runs reuse build outputs. No emulator, native store, relay,
credentials, or real Family data is needed to render the fixtures. The
normal Android SDK/Rust build prerequisites still apply to compilation.
An incremental full render took approximately 15 seconds on the development
machine; this is an observation, not a performance gate.

The **Android debug APK** CI job creates the same gallery on pushes and
PRs. Download `babytrack-android-screen-gallery` from the workflow's artifact
list or job-summary link, unzip it, and open `index.html` locally. This is a
build artifact, not a published website. Artifacts expire according to
GitHub's repository retention setting; rerun CI or the local command to
regenerate them.

Presentation files:

- `MainActivity.kt`: Android entry points, invitation intents, file adapters.
- `TrackerRoute.kt`, `TrackerData.kt`: navigation, data loading, platform and
  Family coordination.
- `TrackerSharingController.kt`, `TrackerBackupController.kt`: sharing and
  enrollment actions, typed foreground sync results, file callbacks, and
  backup/recovery actions using the route's coroutine scope.
- `TrackingActions.kt`: common local/shared write interface and native adapter;
  `ShareCoordinator` implements the shared side through the Rust bindings.
- `CaptureRoute.kt`, `EntryEditController.kt`, `TrackerDrafts.kt`: capture
  and correction actions, with remembered UI drafts and the route's shared
  coroutine scope.
- `TrackerScaffold.kt`: shared target header, bottom navigation, scrolling.
- `TodayScreen.kt`, `HistoryScreen.kt`, `FamilyScreen.kt`, `CaptureScreen.kt`:
  rendering from explicit UI state and named action callbacks.
- `ChildProfileScreen.kt`: fixture-renderable content plus the platform
  dialog/date-picker adapter.
- `EntryDialogs.kt`, `TrackerEditModels.kt`: correction dialog presentation
  and immutable draft values; writes remain in the controller.
- `src/debug/.../ScreenFixtures.kt`: stable fictional display states.
- `src/debug/.../ScreenPreviews.kt`: Android Studio previews of those states.
- `src/test/.../ScreenGalleryTest.kt`: pinned Roborazzi/Robolectric Native
  Graphics renderer, with fixed viewport, locale, time zone, and fixture time.

Add a state to `ScreenFixtures.cases` with a stable ID. The renderer and
previews pick it up automatically. Keep fixtures in the debug source set;
release builds do not include them. Normal unit-test runs skip screenshot
recording; `recordScreenGallery` records the selected fixtures. The PNGs and
`manifest.json` are generated outputs, not committed baselines. Stable IDs,
renderer versions, scroll offsets, and dimensions prepare a later visual
regression comparison; this work does not introduce a pixel-diff gate.
Existing real-relay/UI checks still cover behavior, and phone ergonomics,
TalkBack, keyboard/insets, pseudolocale/RTL, and actual device rendering need
their existing validation paths. See [007](../../docs/tactical/007-android-screen-gallery.md).
