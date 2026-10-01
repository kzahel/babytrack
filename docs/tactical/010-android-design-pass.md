# 010: Android design and daily-use UX pass

Status: active, 2026-10-01. Requested after reviewing the Nara and
Huckleberry research against the Android fixture gallery: the current
screens are text and identical outlined buttons, and the daily surfaces
need a design pass to become palatable before phone validation.

## Goal and exclusions

Give the Android app a recognizable visual system and make the daily
loop — glance at Today, log a feed/diaper/sleep, read the day — fast and
legible. The competitor comparison and its priorities live in
[product feature comparison](../topics/product-feature-comparison.md);
visual decisions are owned by
[interface design](../topics/interface-design-and-localization.md), and
route/screen behavior by [Android navigation](../topics/android-navigation.md).
[005](005-m1-android.md) keeps the physical-phone, accessibility, and
caregiver gates; this workstream does not close them.

Rust continues to own event semantics, day totals, storage, sync, and
crypto. Kotlin adds presentation only: formatting, layout, local UI drafts,
and UI preferences. No protocol, event-type, or storage-schema changes.

Excluded: trend charts and multi-day reports (an explicit scope decision is
still required by the comparison topic), reminders, new activity types,
configurable day boundaries, web parity, iOS, mascot or decorative artwork,
upsell surfaces, and copying any reference app's palette, layout, text,
icons, or assets.

## Ordered delivery slices

1. [x] Record this plan and the owning-topic direction (activity identity
   colors, icon source, capture template, live nursing draft).
2. [x] Foundation: activity category tokens for light and dark with a
   contrast regression; activity icons from the Apache-2.0 AndroidX Material
   Icons library; compact relative/clock time formatting driven by an
   explicit "now"; shared components (activity badge, entry row, section
   header, bottom action bar, choice tiles, single-row chip group).
3. [x] Today: child-first header, live state tiles for feed, sleep, and
   diaper (last event, detail, elapsed sleep clock with Stop), a real empty
   state, recent entries as entry rows, and sync status as a quiet chip.
4. [x] Capture template: activity title and child at the top, a tappable
   time row, the form body, and a full-width bottom Save. Grouped icon grid
   for the activity chooser; icon tiles for diaper; large bottle amount with
   steppers and a repeat-last-amount choice; single-row choice groups.
5. [x] Live nursing and pumping timers: left/right timer buttons with per-side
   elapsed time, pause, last-side hint, and manual entry fallback. The draft
   is UI-local and target-scoped, survives process death, and saves one
   segment list through the existing Rust action, matching the web contract
   in the event model.
6. [x] History: Day/All modes, a week strip, the core day summary for the
   selected day, compact entry rows, a single scrolling filter row, and entry
   actions revealed from the row without a per-row "Details" link.
7. [ ] Family as a settings list: child card, list rows with icons and
   supporting text for children, sharing/access, data and backups, and
   Family options. Developer relay setup stays visibly separate.
8. [ ] Validate and reconcile: unit, lint, both APKs, the fixture gallery
   (dark/light/150%), command-driven UI smoke and recovery checks, and the
   real-relay suite on a disposable emulator; update owning topics, 005, and
   indexes.

## Gates and completion

- Every slice keeps existing actions, target checks, draft retention, and
  saved records unchanged unless the slice says otherwise, and records its
  measured validation before its commit.
- Screens render through the existing fixture gallery with a fixed clock;
  no composable reads the wall clock directly for display state.
- Category colors meet 4.5:1 for text and 3:1 for icons against their
  container in both themes, and color never carries meaning alone.
- Automation labels that tests rely on either remain or are updated in the
  same change as the UI.
- Completion requires all slices delivered, measured checks recorded, the
  gallery re-rendered and inspected, and a clean committed tree. Physical
  phone, TalkBack, RTL, and caregiver gates remain with 005.

## Evidence

- Planning: gallery renders at `78edd97` compared with 139 Nara and 226
  Huckleberry captures in `local-references/`. The Android app had no
  activity icons or category colors, absolute dates on every row, zero-value
  summaries on an empty day, inline Save buttons, and manual-minute nursing.

- Slice 2: `ActivityStyle.kt` maps event kinds to five presentation
  categories, outlined Material icons, and light/dark accent/container
  pairs supplied by `BabytrackTheme`. `TimeDisplay.kt` formats clock,
  yesterday, short-date, elapsed, duration, and stopwatch text from an
  explicit instant and zone; `TrackerComponents.kt` adds the shared badge,
  entry row, section header, choice tiles, chip row, segmented choice, time
  row, bottom action bar, status chip, and settings row. The icon library
  is `material-icons-extended` 1.7.5 from the existing Compose BOM, already
  in the offline Gradle cache. New regressions check every category's
  accent and ink at 4.5:1 on its container in both themes, category
  coverage for every capture kind, and compact time/duration output. All
  Android unit tests and `lintDebug` pass (no errors).

- Slice 3: Today shows the child's name and initial in the top bar (Family
  number only when several Families exist), age with a local/shared status
  chip, explicit warning cards for delayed or blocked sync, and Feeding,
  Sleep, and Diapers tiles with the last entry, its compact time, elapsed
  time, and direct actions. A running sleep shows a live stopwatch, its
  start, and Stop. "So far today" uses the core day summary and an empty
  day shows one line instead of zeros. Entry text moved to a shared
  `entrySummary` used by Today and History. The gallery's five Today cases
  were re-rendered in all four variants and inspected. Unit tests, lint,
  both APKs, the quick caregiver UI smoke on a disposable read-only
  emulator, and the updated Today-summary and header-switch instrumentation
  cases pass. UI scripts now wait for the Feeding tile and the child name
  instead of the former "Family 1 · name" title.

- Slice 4: every capture form shares one template: the activity badge,
  name, and "For <child>" in the top bar; a time row ("Now", or the chosen
  time with Use now and Change time); the form; and a full-width Save in a
  bottom bar that stays above the keyboard. The chooser is a grouped grid of
  category tiles with short names. Diaper uses four icon tiles and an
  explicit Save. Bottle uses a single chip row, a segmented unit choice,
  minus/plus steppers (10 mL or 0.5 fl oz), and a "Same as last" chip from
  the most recent saved bottle. Growth units are segmented in capture and
  correction dialogs. `captureModel` now returns the state and actions so
  the route can place Save outside the scroll area; Rust calls and draft
  rules are unchanged. The 14 capture cases were rendered in all variants
  and inspected (the time row was reflowed after a 150% text check). Unit
  tests, lint, both APKs, the full caregiver UI smoke, and the comma-decimal
  bottle, unsent-draft, and Today-summary instrumentation cases pass.

- Slice 5: Breast feed opens in Timer mode with Enter minutes kept as the
  manual alternative. Tapping a side starts it, tapping the other side
  switches, and tapping the running side pauses; each side shows its own
  elapsed time, the total runs as a large clock, and the side the last
  saved feed ended on is marked. `LiveTimers.kt` keeps the draft in private
  preferences keyed by Family and child, so it survives process death and
  never follows a target switch. Save rounds each timed side to whole
  minutes (at least one), keeps order and pauses, ends no later than now,
  and writes one segment list through the existing Rust action; the
  draft clears only if it is unchanged. Today shows a running or paused
  nursing draft as its own tile with a live clock and Open timer. Pumping
  has a stopwatch that fills minutes and the end time when stopped. Five
  JVM regressions cover tap transitions, the segment limit, rounding and
  ordering, invalid drafts, and per-target persistence. Unit tests, lint,
  both APKs, and the full caregiver UI smoke pass; the smoke now times
  Left, force-stops the app, resumes from Today, switches to Right, saves,
  and finds "Breast · Left 1 min → Right 1 min" in History. Ongoing
  notifications for nursing drafts are not part of this slice.

- Slice 6: History opens on a Day view with a seven-day strip and a date
  picker, the core's day summary for the chosen day (loaded through the
  existing native/shared `daySummary` window, hidden when the day is
  empty), a single scrolling filter row, and day cards of icon rows with
  clock times; an All days view keeps day-grouped headings. Tapping a row
  reveals its existing edit/delete actions, replacing the per-row "Details
  and edits" link. The Day view lists any entry that overlaps the day, so an
  overnight sleep appears on both days its time is counted; this was found
  when the full smoke ran just after midnight. The UI harness expands rows
  by position (with a scroll check so an open row is never collapsed) and
  instrumentation uses an `entry-row` test tag. The disposable emulator
  carried a 320 × 640 dp display override; the full caregiver smoke passes
  there after compacting the summary cells and omitting the redundant
  Today heading. Unit tests, lint, both APKs, the harness regressions, and
  the sleep-move, Undo, diaper time-edit, and header-switch instrumentation
  cases pass.
