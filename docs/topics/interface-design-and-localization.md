# Interface design and localization

Status: M1 direction, with the Android color foundation implemented and the
activity identity and screen templates below being delivered in
[010](../tactical/010-android-design-pass.md). This
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
purple and pink colors.

### Activity identity

Each activity group has one category token pair: an accent and a tonal
container, in light and dark. Feeds (bottle, breast, pumping, solids) use a
warm apricot, sleep a dusk blue, diapers a sage green, health records
(growth, temperature, medication) a muted violet, and notes a neutral sand.
The accent colors an icon inside a small rounded container; a home tile may
use the tonal container as its surface. Do not flood whole screens or Save
buttons with category color: Save, Start, and Stop keep the primary role so
the main action looks the same everywhere. Category colors are not
selection, success, or error colors.

Icons come from the Apache-2.0 AndroidX Material Icons library (outlined
style). Every activity shows its icon with its text label; screen readers
read the label, so the icon is decorative. The 2026-10-01 research review
found both reference apps use per-activity color and iconography to make
the home screen scannable; Babytrack keeps that idea with quieter tonal
surfaces and no illustration or mascot artwork.

Time is shown in the most useful compact form: a clock time for today
("1:35 PM"), "Yesterday" plus time, then a short date without the current
year. Glanceable state on Today uses elapsed time ("25 min ago"). Running
timers show a live elapsed clock. All of these derive from an explicit
current instant supplied to the screen, so fixtures stay deterministic.

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

- **Welcome and first child:** an accountless/privacy headline and a fixed
  bottom **Add your child** action. Omit marketing subtitles, benefit cards,
  and instructions that repeat the controls.
  Join and Restore remain directly reachable. Content scrolls independently
  of the footer at smaller sizes and larger text. The first child form uses
  a name or nickname, a birth date labeled optional, and a
  fixed **Start tracking** button that moves above the keyboard. Other
  profile details are deferred to the normal editor. Failed setup retains
  input and shows a retry explanation on the form; the privacy panel scrolls
  at larger text sizes. Saving disables edits and Back until the local write
  resolves. The owning route and recovery
  behavior are in [Android navigation](android-navigation.md).
- **Today:** the selected child's name and age first, with sharing/sync
  status as a quiet chip that becomes an explicit warning only when work is
  delayed or blocked. Three state tiles — Feed, Sleep, Diaper — show the
  last event's elapsed time and detail; the sleep tile becomes a live
  elapsed clock with Stop while a timer runs. A short row of secondary
  actions (quick wet diaper, all activities) follows. An empty day shows one
  short prompt instead of zero totals. Recent entries use the shared entry
  row and link to History.
- **Capture:** one template for every activity: the activity icon and name
  with the child in the top bar, a tappable time row ("Now" or the chosen
  time), the fields in a simple column, and one full-width Save in a bottom
  bar that stays above the keyboard. Choices use single-row groups or large
  icon tiles, not wrapped chip grids. Show validation beside its field. A
  failed Save leaves input intact. The activity chooser is a grouped icon
  grid (Feeding, Sleep and diapers, Health, Notes).
- **Timers:** a running timer shows a large elapsed clock. Nursing has two
  large side buttons, each with its own elapsed time, plus Pause and a
  last-side hint; manual minute entry remains available.
- **History:** a Day view (default) with a week strip, the day's summary from
  the core, and compact entry rows; an All view keeps the day-grouped list.
  Filters are one scrolling chip row. Each row shows icon, title, detail,
  and time; tapping it reveals that entry's actions.
- **Family:** a settings-style list. A child card (avatar, name, age, edit)
  comes first; rows with an icon, title, and one line of supporting text
  open children, sharing and access, data and backups, and Family options.
  Pending and removed states use specific words and actions rather than a
  generic badge. Developer relay setup stays under Family options.
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

The offline catalog groups cases by destination, with typical states first.
Search and state filters combine with destination selection. Show one dark
100% text preview per case by default; theme and text-size selectors expose
light mode, 150% text, and side-by-side comparisons. A keyboard-accessible
viewer exposes every scroll capture, variant switching, and original PNGs.
State categories are review labels for synthetic fixtures, maintained in the
gallery builder; they do not define application or protocol semantics.

The default review viewport is 412 × 915 dp. Render PNGs at 2× density without
changing the logical layout or font size. A separate optional 360 × 800 dp
compact gallery remains available for constrained-layout checks. Record the
actual viewport, density, font scale, and scroll distance in dp in metadata;
HTML labels derive from those inputs. The command lives in the
[Android README](../../apps/android/README.md#screen-development-and-fixture-gallery).

Keep generated PNGs, rendering metadata, and the offline catalog as local
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
