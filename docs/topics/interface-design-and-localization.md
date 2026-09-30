# Interface design and localization

Status: M1 direction, with the Android color foundation implemented. This
topic owns the visual language and localization rules for product screens.
The [Android navigation topic](android-navigation.md) owns routes and screen
behavior; [005](../tactical/005-m1-android.md) owns delivery evidence. The
[event model](event-model.md) owns stored units, time, and day semantics.

## Direction from the local reference screens

The locally held 2026-09-28 screenshots make common actions easy to spot:
large touch areas, one strong action per capture screen, a clear selected
child, dark surfaces suited to night use, and distinct colors for activity
groups. Use that hierarchy as inspiration. Babytrack should have quieter
surfaces, less visual competition, and no upsell, advice, or decorative
content in the logging path. Do not reproduce the reference app's palette,
screen composition, text, icons, or artwork. Its screenshots contain
private details and are gitignored.

## Visual system

The first Android theme token slice is in `BabytrackTheme.kt`; the widget
uses matching surfaces. Use Material 3 components and these roles rather
than literal colors in individual screens.

| Role | Light | Dark | Use |
| --- | --- | --- | --- |
| Canvas | `#F8F7F3` | `#111C22` | Calm background behind cards and navigation |
| Surface | `#FFFFFF` | `#192A30` | Cards, forms, detail panels |
| Ink | `#18272C` | `#F6F7F4` | Primary text |
| Primary | `#176B75` | `#A5E7E5` | Main Save, start/stop, selected navigation |
| Primary container | `#D3F0EE` | `#24545B` | Quiet selected state and status panel |
| Secondary | `#41636A` | `#AFCED0` | Selected chips and secondary actions |
| Warm accent | `#9E4B30` | `#FFC0A8` | Feed-related illustration/accent |

The Material surface-container roles use neutral green-gray tones, so cards,
chips, and bottom navigation do not fall back to the library's default
purple and pink colors. A future violet care accent can be a category token
when those illustrations exist; it is not a global selection color.

Primary text on primary, ink on surface, and container foregrounds were
chosen at or above 4.5:1 contrast. Future activity colors require the same
check. Color does not carry meaning alone: include an icon and text label
for feed, sleep, diaper, sync, warning, and removal. Error uses the
Material error role, never a category accent. Do not give a pending state
the same treatment as a confirmed success.

Build the new screens on an 8 dp spacing rhythm with generous separation
between activity groups. Use a clear type hierarchy: one screen heading,
short section headings, readable event data, and quieter metadata. Keep
font sizing driven by system settings rather than fixed pixel text. Use
rounded cards and buttons consistently; do not turn every row into a
large card. Primary actions should be visually obvious and reachable by
one hand; destructive actions remain explicit and confirmed. Interactive
targets should be at least 48 dp. Avoid dense side-by-side controls when
labels can grow. Respect reduced motion and never animate status in a way
that obscures whether a write or sync is complete.

### Screen composition

- **Today:** selected target and sync status first; running timer; a small
  number of large, labeled quick actions; compact summary and recent
  entries. Keep the strongest accent for the primary action.
- **Capture:** activity name and target at top; time and fields in a simple
  column; one prominent Save near the bottom. Show validation beside its
  field. A failed Save leaves input intact.
- **History:** readable day headings and compact event rows. Accent plus
  text/icon identifies type; time, amount, and status have separate
  typographic emphasis. Edits open focused detail.
- **Family:** quieter utility layout with clearly separated children,
  access, pending enrollment, data files, and settings. Pending and removed
  states use specific words and actions rather than a generic badge.
- **Child profile:** use a full-height form for create and edit, a simple
  initial avatar, and one reachable Save action. Show name, a locale-formatted
  birth date picker, growth-chart sex, and age derived from the local birth
  day. Use day, week, month, then year units as the child grows. Keep those
  units in plural resources and never store age as separate data.

## Localization contract

English remains the launch language in the [MVP plan](../mvp-plan.md), but
new screens should be translation-ready from their first implementation:

- Put all visible text, accessibility descriptions, notification/widget
  text, and error/status wording in resources. Use positional placeholders
  and Android plurals; never assemble a sentence from translated fragments
  or assume English word order. Give translators context for ambiguous
  words such as “Family,” “left,” and “saved.”
- Let text wrap and controls grow. Avoid fixed widths and single-line
  truncation for action/status labels; check a narrow screen, 1.5× text,
  pseudolocale expansion, and right-to-left layout. Icons that express
  direction should mirror where appropriate; medical or activity symbols
  should not change meaning when mirrored.
- Format dates, times, durations, counts, and measurements for the device
  locale, while keeping the event's saved instant/offset and the viewer's
  day boundary semantics from the event model. Do not assume 12-hour time,
  month/day/year order, or an English “Today” string in generated text.
- Accept a locale's decimal separator in amount inputs, then normalize and
  validate to the canonical decimal passed to Rust. Keep unit choice
  explicit; locale never silently changes an existing event's stored unit
  or value. Display units with localized number formatting and spacing.
  Use date pickers for birth/event dates rather than asking a caregiver to
  type a locale-specific date format.
- Keep protocol identifiers, canonical bytes, invitation links, file
  schema, and diagnostic codes locale-neutral. Translating a label cannot
  change a Family/child target or grant semantics.

Current Android code uses `strings.xml` for most copy, plurals for some
device counts, and locale-aware `DateFormat` for many display times. Birth
dates use the platform date picker and render in the device locale. Bottle,
growth, and temperature fields accept comma or dot decimals and normalize
only at the Rust boundary; saved entered values render with the viewer's
decimal separator. The core's canonical format remains unchanged. Today feed
and diaper counts use Android plurals; remaining count and duration copy
still needs a translation grammar review.

## Fixture gallery workflow

Before redesigning the screens, [007](../tactical/007-android-screen-gallery.md)
establishes a gallery of actual Compose renders. Preserve the current visual
system during extraction. Use fictional data, a fixed clock, locale and time
zone, and stable case IDs. Review dark mode first, alongside light mode and
1.5× text. Show complete scrollable form content as well as viewport context,
so controls below the fold are reviewable.

The default review viewport is 412 × 915 dp, with dark/light 100% text shown
before the 150% accessibility variants. Render PNGs at 2× density without
changing the logical layout or font size. A separate optional 360 × 800 dp
compact gallery remains available for constrained-layout checks. Record the
actual viewport, density, font scale, and scroll distance in dp in metadata;
HTML labels derive from those inputs. The command lives in the
[Android README](../../apps/android/README.md#screen-development-and-fixture-gallery).

Keep generated PNGs, rendering metadata, and a scrolling HTML index as local
build outputs and CI artifacts. Android Studio previews are an additional
entry point into the same fixtures. Start with visual review; introduce a
pixel-comparison gate only after renderer versions and approved baselines
are stable. Use baselines from the same runner platform, SDK, renderer and
JDK; the manifest records these inputs. Small native text anti-aliasing
variations can occur across hosts even with the same screen code; [007](../tactical/007-android-screen-gallery.md)
records the initial comparison. CI artifacts do not require a public gallery
deployment.

## Validation and reconsideration

For each migrated screen, capture light and dark modes at normal and 1.5×
font size, and exercise a pseudolocale/RTL run at least once per navigation
slice. Check field labels, keyboard behavior, status text, accessible names,
and a comma-decimal entry through save and readback. Use representative
non-English strings as layout stress tests even before shipping a second
language. Keep CI smoke focused on a few route and input invariants, with
broader visual review at a milestone checkpoint.

Reconsider the palette or density if a night-use capture is uncomfortable,
contrast or large-text checks fail, or caregivers miss the target/status
before saving. A later platform can adapt native conventions while keeping
the same hierarchy and semantic color roles.
