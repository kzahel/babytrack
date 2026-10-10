<script>
  import { copy as c } from '../strings.js';
  import EntryRow from './EntryRow.svelte';
  import { dayHeading, dayWindow, durationLabel, filters, startOfDay, touchesDay } from '../presentation.js';

  // Android's History: one local day with the core's totals, or every day.
  let { entries = [], nowMs, editable = true, loadSummary, open } = $props();

  let mode = $state('day');
  let filter = $state('all');
  let selected = $state(startOfDay(Date.now()));
  let summary = $state(null);

  const today = $derived(startOfDay(nowMs));
  const day = $derived(Math.min(selected, today));
  const range = $derived(dayWindow(day, nowMs));
  const match = $derived(filters.find((item) => item.id === filter).match);
  const shown = $derived(entries.filter((row) => match(row.kind) && (mode === 'all' || touchesDay(row, range, nowMs))));
  const groups = $derived(mode === 'day' ? [[day, shown]] : [...shown.reduce((map, row) => {
    const key = startOfDay(row.startMs);
    map.set(key, [...(map.get(key) || []), row]);
    return map;
  }, new Map())]);
  // Seven days ending at the later of the chosen day + 3 and today.
  const strip = $derived.by(() => {
    const last = Math.min(today, addDays(day, 3));
    return Array.from({ length: 7 }, (_, index) => addDays(last, index - 6));
  });

  function addDays(ms, count) {
    const date = new Date(ms);
    return new Date(date.getFullYear(), date.getMonth(), date.getDate() + count).getTime();
  }

  let sequence = 0;
  $effect(() => {
    const request = ++sequence;
    const target = range;
    void entries;
    loadSummary(target).then((value) => { if (request === sequence) summary = value; },
      () => { if (request === sequence) summary = null; });
  });

  const isoDay = (ms) => {
    const date = new Date(ms);
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
  };
  const weekday = (ms) => new Intl.DateTimeFormat(undefined, { weekday: 'narrow' }).format(ms);
  const dayNumber = (ms) => new Intl.DateTimeFormat(undefined, { day: 'numeric' }).format(ms);
  const longDay = (ms) => new Intl.DateTimeFormat(undefined, { dateStyle: 'full' }).format(ms);
</script>

<div class="segmented" role="group" aria-label={c.history}>
  <button type="button" aria-pressed={mode === 'day'} onclick={() => mode = 'day'}>{c.dayMode}</button>
  <button type="button" aria-pressed={mode === 'all'} onclick={() => mode = 'all'}>{c.allDays}</button>
</div>
{#if mode === 'day'}
  <div class="week-strip" role="group" aria-label={c.chooseDay}>
    {#each strip as value}
      <button type="button" aria-pressed={value === day} aria-label={longDay(value)} onclick={() => selected = value}>
        <small>{weekday(value)}</small><strong>{dayNumber(value)}</strong>
      </button>
    {/each}
    <label class="day-picker">{c.chooseDay}<input type="date" max={isoDay(today)}
      value={isoDay(day)}
      onchange={(event) => { if (event.currentTarget.value) selected = startOfDay(new Date(`${event.currentTarget.value}T12:00`).getTime()); }} /></label>
  </div>
  {#if day !== today}<h2>{dayHeading(day, nowMs)}</h2>{/if}
{/if}
<div class="filter-row" role="group" aria-label={c.filterLabel}>
  {#each filters as item}
    <button type="button" class="chip" aria-pressed={filter === item.id} onclick={() => filter = item.id}>{item.label}</button>
  {/each}
</div>
{#if mode === 'day' && summary && (summary.feedCount || summary.sleepMs || summary.diaperCount)}
  <div class="day-totals-row panel">
    <div><strong>{c.feedTotal(summary.feedCount)}</strong><span>{summary.bottleMl ? c.bottleTotal(summary.bottleMl) : c.feeds}</span></div>
    <div><strong>{durationLabel(summary.sleepMs)}</strong><span>{c.sleepTotal}</span></div>
    <div><strong>{c.diaperTotal(summary.diaperCount)}</strong><span>{c.wetDirtyTotal(summary.wetDiaperCount, summary.dirtyDiaperCount)}</span></div>
  </div>
{/if}
{#if entries.length === 0}<div class="panel empty">{c.emptyHistory}</div>
{:else if shown.length === 0}<div class="panel empty">{c.emptyView}</div>{/if}
{#each groups as [key, rows] (key)}
  {#if mode === 'all'}<h2>{dayHeading(key, nowMs)}</h2>{/if}
  {#if rows.length}
    <div class="entry-list">
      {#each rows as row (row.id)}
        <EntryRow {row} {editable} {open} />
      {/each}
    </div>
  {/if}
{/each}
