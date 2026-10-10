<script>
  import { copy as c } from '../strings.js';
  import { category, clockTime, entrySummary, iconFor, sleepPlaces } from '../presentation.js';
  import Icon from './Icon.svelte';

  // A history row; tapping it opens the entry's details sheet.
  let { row, editable = true, open } = $props();
</script>

<div class="entry">
  <span class="entry-dot {category(row.kind)}"><Icon name={iconFor(row.kind)} size={22} /></span>
  <button class="entry-main" disabled={!editable} onclick={() => open(row)}>
    <strong>{entrySummary(row)}</strong>
    {#if row.kind !== 'note' && row.note}<small>{c.noteDetail(row.note)}</small>{/if}
    {#if row.kind === 'sleep' && sleepPlaces[row.sleepPlace]}<small>{c.place(sleepPlaces[row.sleepPlace])}</small>{/if}
  </button>
  <time>{clockTime(row.startMs)}</time>
</div>
