<script>
  import { copy as c } from '../strings.js';
  import { sideTotals } from '../breast-timer.js';
  import Icon from './Icon.svelte';
  import { category, clock, clockTime, compactDateTime, diaperKinds, durationLabel, entrySummary, feedDetail,
    iconFor, isRunningSleep, latest, nextBreastSide, ribbon, shortElapsed, startOfDay, dayWindow } from '../presentation.js';

  // The approved Today: what is running now, time since the last feed,
  // sleep, and diaper, six one-tap actions, the day ribbon, and recent entries.
  let { entries = [], summary = null, breastDraft = null, pumpStartMs = null, nowMs, busy = false,
    open, startSleep, openSleep, logDiaper, viewHistory } = $props();

  const lastFeed = $derived(latest(entries, (kind) => ['feed.breast', 'feed.bottle', 'feed.solids'].includes(kind)));
  const runningSleep = $derived(entries.filter(isRunningSleep).reduce((best, row) =>
    (!best || row.startMs > best.startMs ? row : best), null));
  const lastSleep = $derived(entries.filter((row) => row.kind === 'sleep' && row.endMs != null)
    .reduce((best, row) => (!best || row.endMs > best.endMs ? row : best), null));
  const lastDiaper = $derived(latest(entries, (kind) => kind === 'diaper'));
  const lastBottle = $derived(latest(entries.filter((row) => row.bottleEntered != null || row.bottleMl != null),
    (kind) => kind === 'feed.bottle'));
  const diapersToday = $derived(entries.filter((row) => row.kind === 'diaper' && row.startMs >= startOfDay(nowMs)).length);
  const nextSide = $derived(nextBreastSide(entries));
  const nursing = $derived(breastDraft ? sideTotals(breastDraft, nowMs) : null);
  const day = $derived(ribbon(entries, dayWindow(nowMs, nowMs), nowMs));
  const emptyDay = $derived(!runningSleep && (!summary || (!summary.feedCount && !summary.sleepMs && !summary.diaperCount)));
  const sideName = (side) => (side === 1 ? c.left : c.right);
</script>

{#if runningSleep}
  <button class="now-card" onclick={() => openSleep(runningSleep)}>
    <span><small>{c.sleepingSince(clockTime(runningSleep.startMs))}</small><strong>{clock(nowMs - runningSleep.startMs)}</strong></span>
    <Icon name="chevron" size={22} />
  </button>
{/if}
{#if nursing}
  <button class="now-card" onclick={() => open('feed.breast')}>
    <span><small>{breastDraft.active ? c.breastfeedingSide(sideName(breastDraft.active.side)) : c.breastfeedingPaused}</small>
      <strong>{clock(nursing[1] + nursing[2])}</strong></span>
    <Icon name="chevron" size={22} />
  </button>
{/if}
{#if pumpStartMs}
  <button class="now-card" onclick={() => open('pump')}>
    <span><small>{c.pumpingNow}</small><strong>{clock(nowMs - pumpStartMs)}</strong></span>
    <Icon name="chevron" size={22} />
  </button>
{/if}

<div class="since">
  <div class="since-card feed">
    <span class="since-label"><i></i>{c.lastFeed}</span>
    <strong>{lastFeed ? shortElapsed(nowMs - lastFeed.startMs) : '—'}</strong>
    <small>{lastFeed ? feedDetail(lastFeed) : c.noneYet}</small>
  </div>
  <div class="since-card sleep">
    {#if runningSleep}
      <span class="since-label"><i></i>{c.asleep}</span>
      <strong>{shortElapsed(nowMs - runningSleep.startMs)}</strong>
      <small>{c.sinceTime(clockTime(runningSleep.startMs))}</small>
    {:else}
      <span class="since-label"><i></i>{c.awake}</span>
      <strong>{lastSleep ? shortElapsed(nowMs - lastSleep.endMs) : '—'}</strong>
      <small>{lastSleep ? c.napped(durationLabel(lastSleep.endMs - lastSleep.startMs)) : c.noneYet}</small>
    {/if}
  </div>
  <div class="since-card care">
    <span class="since-label"><i></i>{c.lastDiaper}</span>
    <strong>{lastDiaper ? shortElapsed(nowMs - lastDiaper.startMs) : '—'}</strong>
    <small>{lastDiaper ? c.diaperToday(diaperKinds[lastDiaper.diaperKind], diapersToday) : c.noneYet}</small>
  </div>
</div>

<div class="actions-grid">
  <button class="action feed" onclick={() => open('feed.bottle')}><Icon name="bottle" size={30} />
    <span>{c.bottle}<small>{lastBottle ? c.lastAmount(feedDetail(lastBottle).replace(/ bottle$/, '')) : c.logBottle}</small></span></button>
  <button class="action feed" onclick={() => open('feed.breast')}><Icon name="breast" size={30} />
    <span>{c.breast}<small>{nextSide ? c.startSide(sideName(nextSide)) : c.startTimer}</small></span></button>
  <button class="action care" disabled={busy} onclick={() => logDiaper(1)}><Icon name="wet" size={30} />
    <span>{c.wet}<small>{c.logNow}</small></span></button>
  <button class="action care" disabled={busy} onclick={() => logDiaper(2)}><Icon name="dirty" size={30} />
    <span>{c.dirty}<small>{c.logNow}</small></span></button>
  <button class="action sleep" disabled={busy} onclick={() => (runningSleep ? openSleep(runningSleep) : startSleep())}><Icon name="sleep" size={30} />
    <span>{c.sleep}<small>{runningSleep ? c.openTimer : c.startTimer}</small></span></button>
  <button class="action neutral" onclick={() => open(null)}><Icon name="plus" size={30} />
    <span>{c.moreActivities}<small>{c.moreHint}</small></span></button>
</div>

<section class="day-card" aria-labelledby="day-heading">
  <h2 id="day-heading">{c.today}<span>{c.sinceMidnight}</span></h2>
  <div class="ribbon" role="img" aria-label={c.dayRibbon}>
    {#each day.sleeps as item}<span class="ribbon-sleep" style:left="{item.left}%" style:width="{item.width}%"></span>{/each}
    {#each day.feeds as left}<span class="ribbon-feed" style:left="{left}%"></span>{/each}
    {#each day.diapers as left}<span class="ribbon-diaper" style:left="{left}%"></span>{/each}
    <span class="ribbon-future" style:left="{day.now}%"></span>
    <span class="ribbon-now" style:left="{day.now}%"></span>
  </div>
  <div class="ribbon-ticks" aria-hidden="true"><span>12a</span><span>6a</span><span>12p</span><span>6p</span><span>12a</span></div>
  {#if emptyDay}<p class="muted day-empty">{c.nothingToday}</p>
  {:else if summary}
    <div class="day-totals-row">
      <div><strong>{c.feedTotal(summary.feedCount)}</strong><span>{c.bottleTotal(summary.bottleMl)}</span></div>
      <div><strong>{durationLabel(summary.sleepMs)}</strong><span>{c.sleepTotal}</span></div>
      <div><strong>{c.diaperTotal(summary.diaperCount)}</strong><span>{c.wetDirtyTotal(summary.wetDiaperCount, summary.dirtyDiaperCount)}</span></div>
    </div>
  {/if}
</section>

<div class="section-heading"><h2>{c.recent}</h2><button class="text-action" onclick={viewHistory}>{c.viewTimeline}</button></div>
{#if entries.length === 0}<p class="muted">{c.emptyHistory}</p>
{:else}
  <div class="recent-list">
    {#each entries.slice(0, 3) as row (row.id)}
      <button class="recent-row" onclick={viewHistory}>
        <span class="recent-dot {category(row.kind)}"><Icon name={iconFor(row.kind)} size={18} /></span>
        <strong>{entrySummary(row)}</strong><span>{compactDateTime(row.startMs, nowMs)}</span>
      </button>
    {/each}
  </div>
{/if}
