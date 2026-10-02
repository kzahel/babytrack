# 011: Android local timer notifications

Status: complete with local verification, 2026-10-02. Opened 2026-10-02. Requested after the
[010 design pass](010-android-design-pass.md) to make its nursing and pumping
drafts visible while the app is closed.

## Goal and exclusions

Show quiet, private notifications for running nursing and pumping timers,
keep paused nursing visible with a frozen duration, and return a notification
tap to the exact saved Family, child, and draft session. Preserve timing when
permission is denied or the notification is dismissed. The
[Android navigation topic](../topics/android-navigation.md) owns behavior;
the [event model](../topics/event-model.md#timers) owns local versus shared
timer semantics. Phone and accessibility gates remain with [005](005-m1-android.md).

Excluded: notification actions that change a timer or save an entry,
foreground services, reminders, push, live shared nursing/pumping timers,
sleep notification/widget redesign, protocol changes, and hosted preview work.

## Ordered delivery slices

1. [x] Add per-target/per-type notification identities, system chronometers,
   private public-version copy, pause display, session-scoped dismissal, and
   one contextual notification permission request.
2. [x] Resolve cold/warm taps through current core Family/child state and the
   exact persisted session; refresh after transitions, permission grant,
   foreground return, reboot, and scheduled sync.
3. [x] Validate unit/lint/build checks, actual notification navigation and
   cleanup on an emulator, force-stop/reopen and offline boot recovery,
   and existing sleep/caregiver regressions. Inspect system notification UI.
4. [x] Record measured evidence and update owning topics and indexes; commit
   the finished slice on main.

## Gates and completion

- No notification writes canonical events or retargets a draft.
- Nursing uses active segment time, excluding pauses; paused time is static.
- Dismissal does not discard a draft and ordinary refresh does not repost the
  dismissed session. A later session remains eligible for a notification.
- Save clears nursing visibility only after success; pumping Stop removes
  its running notification and retains the existing form-based save flow.
- Multiple targets/types coexist without overwriting each other's intents.
- Permission denial leaves logging/timing usable. Force-stop recovery means
  restoration on user reopening, not notifications while the app is stopped.
- Complete when checks and observed limits are recorded and committed.

## Evidence

Measured locally on 2026-10-02 on disposable, read-only API 35 emulators:

| Check | Result |
| --- | --- |
| Android JVM suite | Pass: 24 tests, including 7 new notification regressions and API 26 coverage |
| `lintDebug` and both APK builds | Pass: no lint errors |
| Focused timer instrumentation | Pass: cold/warm exact-target taps, Activity recreation, independent Stop/Save cleanup, paused boot refresh and dismissal, rejected Save retention, stale-session rejection |
| Default real-relay runner | Pass: clean-install invite plus all 32 sharing/timer cases; now includes the timer class in CI |
| Shared recipient notification | Pass: local cached child name/target resolves; verified removal hides the notification and retains its original-target draft |
| Actual permission dialog | Pass: decline, switch sides without another prompt, grant through the platform permission manager and return to foreground |
| Ordinary process death | Pass: terminate the background app process without force-stop; notification remains and a system-drawer tap opens the unchanged draft |
| Actual offline emulator reboot | Pass: Wi-Fi/mobile data disabled; both nursing and pumping drafts and notifications restore before opening the app; exact preference state is unchanged |
| System notification UI | Running and paused captures inspected; clocks and labels fit and appear under Silent |
| Full caregiver UI smoke | Pass: local setup, target switching, logging/editing, nursing notification appearance, force-stop/reopen restoration, and successful Save cleanup at the inherited 320 × 640 dp / 1.5× text setting |
| Harness regressions, workspace boundary and diff checks | Pass |

System captures and the local lifecycle result are retained in gitignored
`local-references/android-timer-notifications-qa/`. The existing UIAutomator
idle-wait dump could not read the continuously updating system chronometer;
posted-state assertions and inspected system-drawer taps supplied that
lifecycle evidence. Instrumentation separately exercises the real pending
intents and their cold/warm route behavior.

The notification navigation check found that Activity recreation closed the
restored capture route. Target reset now closes capture only on an actual
Family/child change; the focused recreation and cross-target checks pass.

No hosted-service check, physical-phone/accessibility gate, or remote CI run
is claimed. Phone reboot, lock-screen settings, TalkBack, and caregiver use
remain with 005. Notifications do not add background timer controls or live
cross-device nursing/pumping state.
