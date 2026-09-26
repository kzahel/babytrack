# Product and business opportunity proposal

Working name. A free, open source (MIT), end-to-end encrypted baby tracker with
first-party hosted sync, native apps on both stores and F-Droid, a web client, and watch
apps for Apple Watch and Wear OS. Self-hostable in one command.

Status: background proposal, September 2026. Preserved for product and
business rationale; this is not an implementation contract. Current scope
and decisions live in [mvp-plan.md](mvp-plan.md) and the
[topics](topics/README.md). Read this document for strategy work, not routine
implementation. Market claims below are historical planning estimates, not
revalidated facts.

Known qualifications when reading this snapshot:

- "Every feature free" and the later paid watch/widget ideas are unresolved
  scope/pricing alternatives; see [M-1](tactical/001-pre-m0-design.md).
- WHO/CDC in this proposal is broader than the current WHO direction in the
  [event model](topics/event-model.md). The medical-content boundary still
  needs scope closure; the proposal is not permission to add clinical advice.
- Absolute privacy claims such as "nothing to leak" must be read against
  the [accepted trust limits](topics/family-sharing-and-trust.md#accepted-trust-limits),
  including relay metadata and hosted web-code trust.
- "Cents per family per year" is a cost hypothesis, not a measured budget.

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
- Pumping with volume; proposed freezer inventory for stored milk portions
- Diapers (wet, dirty, both)
- Growth (weight, length, head) with WHO/CDC percentiles
- Medication and temperature with dose intervals
- Notes, multiple children, twins done properly

Platforms: iOS, Android, Apple Watch, Wear OS, web. All are first-class and
all share one encrypted data model.

The app starts with a local-only Family, with no account or server, unless the
person chooses to join an invitation on first launch. A **Family** is an
independent space for children, entries, and sharing; it need not match a
biological or legal family. One app can hold several Families and lets a
caregiver switch between them. Sharing a local Family is an explicit choice
that includes its existing history. A shared Family supports unlimited
caregivers: members can read and write entries, while managers can
also invite, remove, or change another person's role, including a manager's.
At least one manager must remain. All members keep the history already stored
on their devices and can make an independent local copy. Sync works offline
and merges distinct entries from caregivers.

If two managers try to remove each other, the first valid removal committed
by the relay takes effect in the original Family. Both keep their local work
and may choose independent copies; the app does not automatically split the
Family or redirect other caregivers. The
[Family sharing contract](topics/family-sharing-and-trust.md) records
these user flows, trust limits, and the remaining UX choices.

Invitations authorize one direct join, without a second manual approval.
Once used, the link cannot enroll another device; there are no perpetual
membership links. Anyone who obtains a valid unused invitation may use it
first, so it should be shared only with the intended recipient.

Import: one-tap import from Huckleberry, Nara, and Baby Tracker (Nighp) CSV
exports. Export: one tap, open format, always available.

A complete Family file can be saved and restored, with optional password
protection; readable exports also support analysis in other programs.
Restoring creates a new local-only Family with the saved children and data.
It works after phone loss or loss of shared access without rejoining the
original group. The person can share the new Family and invite caregivers
again. A file restores only the data actually saved, not the original
Family's membership or device credentials.

Merging two live Families or automatically combining child histories is out
of the MVP. Anyone may backfill an activity with its original date. A later
client-side import tool could copy selected data into another Family without
merging membership or sync histories.
A possible later CLI could expose that import with explicit child mapping
and duplicate/conflict handling. It is not required for the MVP.

## Architecture principles

- **Local-first.** The phone is the source of truth. A person can create a
  local-only Family, fully functional without an account or network, or join
  an invitation on first launch. Sharing is opt-in for each local Family.
- **End-to-end encrypted sync.** The server is a dumb relay for encrypted
  blobs. A QR code or invite link bootstraps access; later epoch keys are
  wrapped for each holder and relayed by the server. Wear OS can hold the
  shared core and keys; the MVP Apple Watch app relays through the phone.
  The web invite uses a link fragment. We cannot read user data.
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
- E2EE makes key recovery hard. A shared family's relay copy is unusable
  after all keys are lost; recovery needs another device, a backed-up key,
  or a recovery phrase.
- A sole local-only device can lose its records if it is lost or damaged;
  a recovery phrase without a copy of the records is not a backup.
- Store review friction for health-adjacent apps (Enfold was rejected once
  over medical citations). Keep medical content out of the MVP.
- Crowded 2026 field means "free and open" is table stakes. Distribution and
  logging speed decide it.
- Solo maintenance burden across five platforms. Sharing the sync and crypto
  engine is the leverage point. Watch apps are three screens each: start/stop
  timers, one-tap diaper, last-event complication.

## Open questions

The architecture direction in [docs/mvp-plan.md](mvp-plan.md) is a
shared Rust core and append-only operation log. Protocol, recovery, and MVP
scope details must be settled in
[M-1 design closure](tactical/001-pre-m0-design.md) before implementation.
Freezer inventory would track individual portions of expressed milk: amount,
storage date, and whether each portion was used or discarded. Its MVP status
is open. Also open:

- Final name. `babytrack` is the code name until publishing.
