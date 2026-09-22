# babytrack: product and business opportunity proposal

Working name. A free, open source (MIT), end-to-end encrypted baby tracker with
first-party hosted sync, native apps on both stores, a web client, and watch
apps for Apple Watch and Wear OS. Self-hostable in one command.

Status: proposal, September 2026. Nothing built yet.

## Why now

Nara Baby, for years the reference "completely free, no ads" tracker with a
claimed 1.5M+ users, moved to a paid model in 2026: 7-day trial, then $9.99/mo
or $99.99 lifetime, and the whole family's data goes read-only when the payer
lapses. Parents are actively looking for a replacement and a wave of new
entrants launched in 2026 promising "free and private." None of them has
distribution, a watch app, a web client, or a credible reason to believe the
hosted service will still exist in two years.

## Core product (MVP)

Activity logging, fast enough to use one-handed at 3am:

- Feeding: breast (side, timer), bottle (amount), solids
- Sleep and naps with running timers and wake-window hints
- Pumping with volume and a freezer inventory
- Diapers (wet, dirty, both)
- Growth (weight, length, head) with WHO/CDC percentiles
- Medication and temperature with dose intervals
- Notes, multiple children, twins done properly

Platforms: iOS, Android, Apple Watch, Wear OS, web. All are first-class and
all share one encrypted data model.

Sharing: unlimited caregivers per family, forever. Realtime sync between
caregivers with offline support and conflict-free merge (append-only event log
or CRDT), so two parents logging the same feed never lose data.

Import: one-tap import from Huckleberry, Nara, and Baby Tracker (Nighp) CSV
exports. Export: one tap, open format, always available.

## Architecture principles

- **Local-first.** The phone is the source of truth. The app is fully
  functional with no account and no network.
- **End-to-end encrypted sync.** The server is a dumb relay for encrypted
  blobs. The family key is shared by QR or invite link, never through the
  server. Watches get the key from the phone over WatchConnectivity (watchOS)
  or the Data Layer API (Wear OS). Web gets it via a link fragment. We cannot read user data.
- **Notifications without plaintext.** Server sends empty wake pushes; the
  client decrypts and renders. Reminders (wake windows, feed intervals) are
  scheduled on-device from local data.
- **Self-hostable.** The whole backend is one small service plus a database,
  one `docker compose up`. The client has a "custom server" setting.
- **Tiny.** Native clients, no cross-platform UI framework. Target well under
  30 MB and sub-second cold start. Most competitors are 100 to 180 MB.
- **Nearly free to run.** Encrypted blob relay costs cents per family per year,
  which is what makes "free forever" an honest claim.

## Differentiators

Against the paid incumbents (Huckleberry, Nara, Glow, Napper):

1. Free with every feature, no trial, no read-only lockout, ever.
2. Private by construction, not by policy. We can't see the data, so there is
   nothing to sell, leak, or lose in an acquisition.
3. Web client. Almost nobody in the category offers one.
4. Faster logging: widgets, lock screen timers, watch complications and
   tiles, Siri and Assistant shortcuts.
5. Durable: works if we disappear, exports in one tap, self-host if you want.

Against the 2026 open source entrants (Enfold, Beanlo, Finnberry, Baby Buddy):

1. On both stores on day one with a hosted service we commit to running.
   Self-host-only projects never reach a sleep-deprived parent searching the
   App Store.
2. Watch apps on both Apple Watch and Wear OS. None of them have either.
3. E2EE. Only one tiny project has attempted it and it is single-user.
4. MIT rather than AGPL. Friendlier signal, no rebrand covenants.
5. Native and small rather than Flutter, Next.js, or Firebase.

## Market

Third-party estimates, treat as directional.

| App | Model | iOS ratings | Play installs | Est. revenue |
|---|---|---|---|---|
| Huckleberry | Freemium, $12 to $15/mo | 73K | 1M+ | ~$700K to 800K/mo, ~6:1 iOS:Android |
| Baby Tracker (Nighp) | Free core, $30/yr | 227K | 1M+ | Android near zero |
| Nara Baby | Paid since 2026, $10/mo or $100 lifetime | 24K | 100K+ | Not public; 1.5M+ lifetime users claimed |
| Baby Daybook | Freemium, ~$30 lifetime | 3K | 1M+ | ~$30K/mo (Oct 2024 est.) |
| Glow Baby | Ads plus $10/mo | 23K | 1M+ | Not public |
| Napper | $70/yr, sleep focus | n/a | 1M+ | Not public |

Category notes:

- Baby tracker apps are estimated at roughly $0.5B to $1B globally, North
  America about 38%, but that figure includes monitors and content apps. The
  pure logging niche is modest.
- Revenue skews heavily to iOS. Huckleberry's split is roughly 6:1 on revenue
  and 4:1 on downloads. Nighp has 1M+ Android installs and earns almost nothing
  there.
- Every user churns within 12 to 24 months. Subscriptions fit the category
  badly and are resented. One-time and lifetime pricing fits well.
- Huckleberry is the only entrant with real money ($16M raised, ~64 staff)
  and it wins on paid sleep predictions, not on logging.

## Business model

Honest framing: this is a weak standalone business and a good product with a
plausible modest income, low running cost, and option value.

Phase 1, free only. Build goodwill and users. Publicly commit that the free
tier is complete and permanent, and announce in advance that a paid unlock for
extras will come later. No surprises.

Phase 2, lifetime unlock. One-time $20 to $40 for convenience extras that do
not gate the core: watch apps, widgets, PDF reports, daycare share links,
twin mode niceties. Baby Daybook does this at ~$30 and is estimated at ~$30K/mo
after roughly seven years of installs. That is the realistic ceiling for a
solo indie here, not a year-two projection.

Not doing: subscriptions, ads, selling data (impossible by design), paid sync
tiers.

Upside cases: acquisition by a hardware or formula brand, a pediatric
telehealth company, or a monitor maker who wants the daily touchpoint. MIT
does not hurt this since they would buy users and brand, not code. Realistic
scale is low seven figures at best.

## Voice and AI (deliberately TBD)

Voice logging ("she pooped five minutes ago") is the one AI feature that speeds
up the core loop rather than decorating it. Plan:

1. Siri and Assistant shortcuts via App Intents first. No model, works from
   the watch, zero cost.
2. On-device speech to text, then a grammar and regex parser for the small
   intent space (event type, side, amount, duration, relative time). Covers
   most utterances with no model at all.
3. A small on-device model (sub-1B, fine-tuned on synthetic utterance-to-JSON
   data) for the long tail. Keeps the E2EE story intact.
4. Optional bring-your-own endpoint (Ollama or similar) for self-hosters.
5. Opt-in, clearly labeled collection of corrected transcripts to improve the
   model, if this ever gets serious.

Hosted classifier APIs (evaluated TypeSafe's Jev: fast, cheap, classification
only, hosted only) do not fit the privacy story and are not needed. Cloud AI
chat, sleep coaching, articles, and white noise are explicitly out of scope.

## Competitive landscape reference

Commercial: Huckleberry, Nara Baby, Baby Tracker (Nighp), Glow Baby, Baby
Daybook, Napper, Baby Connect, Talli (hardware), BabyCenter (content).

Open source, 2026 entrants with hosted service: Enfold (Flutter, FastAPI,
AGPL, iOS live Sep 18 2026, 0 ratings), Beanlo (Next.js, Supabase, AGPL,
TestFlight only).

Open source, self-host only: Baby Buddy (Django, BSD, 2.9K stars, the
incumbent for homelabbers, third-party Android and iOS companions), Finnberry
(Next.js, Supabase, MIT), mbentancour/babytracker (Go single binary, MIT),
shnick92/baby-tracker (React PWA, two-parent only, MIT).

Not really open: Sprout Track (custom non-commercial license), Sara (GPL but
Firebase backend).

None of the open source projects has a watch app on either platform. None has E2EE with
multi-caregiver sync.

## Risks

- Category economics cap the upside. Enter for the product, the infra
  exercise, and option value, not for income.
- E2EE makes account recovery hard. Lose all devices and lose the data unless
  we add a recovery phrase or key escrow. Needs a careful, simple answer.
- Store review friction for health-adjacent apps (Enfold was rejected once
  over medical citations). Keep medical content out of the MVP.
- Crowded 2026 field means "free and open" is table stakes. Distribution and
  logging speed decide it.
- Solo maintenance burden across five platforms. Sharing the sync and crypto
  engine is the leverage point. Watch apps are three screens each: start/stop
  timers, one-tap diaper, last-event complication.

## Open questions

- Native stack choice: Swift plus Kotlin with a shared core (Kotlin
  Multiplatform or Rust) versus two fully independent apps.
- Sync design: append-only event log with client merge versus a CRDT library.
- Key recovery UX.
- Name.
