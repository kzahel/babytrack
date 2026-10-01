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
2. [ ] Foundation: activity category tokens for light and dark with a
   contrast regression; activity icons from the Apache-2.0 AndroidX Material
   Icons library; compact relative/clock time formatting driven by an
   explicit "now"; shared components (activity badge, entry row, section
   header, bottom action bar, choice tiles, single-row chip group).
3. [ ] Today: child-first header, live state tiles for feed, sleep, and
   diaper (last event, detail, elapsed sleep clock with Stop), a real empty
   state, recent entries as entry rows, and sync status as a quiet chip.
4. [ ] Capture template: activity title and child at the top, a tappable
   time row, the form body, and a full-width bottom Save. Grouped icon grid
   for the activity chooser; icon tiles for diaper; large bottle amount with
   steppers and a repeat-last-amount choice; single-row choice groups.
5. [ ] Live nursing and pumping timers: left/right timer buttons with per-side
   elapsed time, pause, last-side hint, and manual entry fallback. The draft
   is UI-local and target-scoped, survives process death, and saves one
   segment list through the existing Rust action, matching the web contract
   in the event model.
6. [ ] History: Day/All modes, a week strip, the core day summary for the
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
