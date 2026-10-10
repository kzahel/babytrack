<script>
  import { copy as c } from '../strings.js';
  import { sideTotals } from '../breast-timer.js';
  import { clock, clockTime, compactDateTime, durationLabel, elapsedLabel, entrySummary,
    isRunningSleep, latest } from '../presentation.js';

  // Android's Today: state tiles for the daily loop, the core's day totals,
  // and the newest entries. Every write goes through the parent's actions.
  let { entries = [], summary = null, breastDraft = null, nowMs, busy = false, open, startSleep,
    stopSleep, wetNow, viewHistory } = $props();

  const lastFeed = $derived(latest(entries, (kind) => ['feed.breast', 'feed.bottle', 'feed.solids'].includes(kind)));
  const runningSleep = $derived(entries.filter(isRunningSleep).reduce((best, row) =>
    (!best || row.startMs > best.startMs ? row : best), null));
  const lastSleep = $derived(entries.filter((row) => row.kind === 'sleep' && row.endMs != null)
    .reduce((best, row) => (!best || row.endMs > best.endMs ? row : best), null));
  const lastDiaper = $derived(latest(entries, (kind) => kind === 'diaper'));
  const nursing = $derived(breastDraft ? sideTotals(breastDraft, nowMs) : null);
  const emptyDay = $derived(!runningSleep && (!summary || (!summary.feedCount && !summary.sleepMs && !summary.diaperCount)));
</script>

<div class="tiles">
  <section class="tile feed" aria-labelledby="feed-tile">
    {#if nursing}
      <h2 id="feed-tile">{breastDraft.active ? c.breastInProgress : c.breastPaused}</h2>
      <strong class="tile-clock">{clock(nursing[1] + nursing[2])}</strong>
      <p>{c.leftRight(clock(nursing[1]), clock(nursing[2]))}</p>
      <div class="tile-actions"><button class="primary" onclick={() => open('feed.breast')}>{c.openTimer}</button></div>
    {:else}
      <div class="tile-head"><h2 id="feed-tile">{c.groupFeeding}</h2>
        {#if lastFeed}<span>{elapsedLabel(nowMs - lastFeed.startMs)}</span>{/if}</div>
      <p>{lastFeed ? c.entry(entrySummary(lastFeed), compactDateTime(lastFeed.startMs, nowMs)) : c.noFeeds}</p>
      <div class="tile-actions">
        <button class="primary" onclick={() => open('feed.bottle')}>{c.bottle}</button>
        <button class="secondary" onclick={() => open('feed.breast')}>{c.breastFeedButton}</button>
      </div>
    {/if}
  </section>
  <section class="tile sleep" aria-labelledby="sleep-tile">
    {#if runningSleep}
      <h2 id="sleep-tile">{c.sleeping}</h2>
      <strong class="tile-clock">{clock(nowMs - runningSleep.startMs)}</strong>
      <p>{c.since(clockTime(runningSleep.startMs))}</p>
      <div class="tile-actions"><button class="primary" disabled={busy} onclick={() => stopSleep(runningSleep)}>{c.stopSleep}</button></div>
    {:else}
      <div class="tile-head"><h2 id="sleep-tile">{c.sleep}</h2>
        {#if lastSleep}<span>{elapsedLabel(nowMs - lastSleep.endMs)}</span>{/if}</div>
      <p>{lastSleep ? c.slept(durationLabel(lastSleep.endMs - lastSleep.startMs), clockTime(lastSleep.endMs)) : c.noSleep}</p>
      <div class="tile-actions">
        <button class="primary" disabled={busy} onclick={() => startSleep()}>{c.startSleep}</button>
        <button class="secondary" onclick={() => open('sleep')}>{c.addPastSleep}</button>
      </div>
    {/if}
  </section>
  <section class="tile care" aria-labelledby="diaper-tile">
    <div class="tile-head"><h2 id="diaper-tile">{c.diapers}</h2>
      {#if lastDiaper}<span>{elapsedLabel(nowMs - lastDiaper.startMs)}</span>{/if}</div>
    <p>{lastDiaper ? c.entry(entrySummary(lastDiaper), compactDateTime(lastDiaper.startMs, nowMs)) : c.noDiapers}</p>
    <div class="tile-actions">
      <button class="primary" disabled={busy} aria-label={c.wetNowLabel} onclick={wetNow}>{c.wetNow}</button>
      <button class="secondary" onclick={() => open('diaper')}>{c.logDiaper}</button>
    </div>
  </section>
</div>
<button class="secondary add-activity" onclick={() => open(null)}>＋ {c.addActivityTitle}</button>

<h2>{c.soFarToday}</h2>
<div class="summary panel">
  {#if emptyDay}<p>{c.nothingToday}</p>
  {:else if summary}
    <p>{c.feedLine(summary.feedCount, summary.bottleMl)}</p>
    <p>{c.sleepLine(durationLabel(summary.sleepMs))}</p>
    <p>{c.diaperLine(summary.diaperCount, summary.wetDiaperCount, summary.dirtyDiaperCount)}</p>
  {/if}
</div>
<div class="section-heading"><h2>{c.recent}</h2><button class="text-action" onclick={viewHistory}>{c.viewTimeline}</button></div>
{#if entries.length === 0}<p class="muted">{c.emptyHistory}</p>{/if}
{#each entries.slice(0, 3) as row (row.id)}
  <button class="recent-row" onclick={viewHistory}><strong>{entrySummary(row)}</strong><span>{compactDateTime(row.startMs, nowMs)}</span></button>
{/each}
