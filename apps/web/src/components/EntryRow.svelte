<script>
  import { copy as c } from '../strings.js';
  import { category, clockTime, entrySummary, iconFor, isRunningSleep, sleepPlaces } from '../presentation.js';
  import Icon from './Icon.svelte';

  // A history row; tapping it reveals the corrections Android offers for that type.
  let { row, editable = true, expanded = false, toggle, edit, stopSleep, remove } = $props();

  const instant = ['note', 'feed.bottle', 'feed.solids', 'diaper', 'growth', 'medication', 'temperature'];
  const actions = $derived([
    !isRunningSleep(row) && ['sleep', 'pump'].includes(row.kind) && row.endMs > row.startMs && ['move', c.moveSession],
    instant.includes(row.kind) && ['time', c.editTime],
    row.kind === 'sleep' && !isRunningSleep(row) && ['sleep-end', c.editSleepDuration],
    row.kind === 'sleep' && ['sleep-place', c.editSleepPlace],
    row.kind === 'feed.bottle' && row.bottleMl != null && ['bottle', c.editBottle],
    row.kind === 'feed.breast' && row.breastSegments?.length && ['breast', c.editBreast],
    row.kind === 'diaper' && ['diaper', c.editDiaperType],
    row.kind === 'feed.solids' && ['solids', c.editSolids],
    row.kind === 'pump' && ['pump', c.editPumpAmounts],
    row.kind === 'medication' && row.medicationName && ['medication', c.editMedication],
    row.kind === 'growth' && ['growth', c.editGrowth],
    row.kind === 'temperature' && row.temperatureC != null && ['temperature', c.editTemperature],
    ['note', row.kind === 'note' || row.note ? c.editNote : c.addNote],
  ].filter(Boolean));
</script>

<div class="entry">
  <span class="entry-dot {category(row.kind)}"><Icon name={iconFor(row.kind)} size={22} /></span>
  <button class="entry-main" aria-expanded={editable ? expanded : undefined} disabled={!editable} onclick={toggle}>
    <strong>{entrySummary(row)}</strong>
    {#if row.kind !== 'note' && row.note}<small>{c.noteDetail(row.note)}</small>{/if}
    {#if row.kind === 'sleep' && sleepPlaces[row.sleepPlace]}<small>{c.place(sleepPlaces[row.sleepPlace])}</small>{/if}
  </button>
  <time>{clockTime(row.startMs)}</time>
  {#if editable && expanded}
    <div class="entry-actions">
      {#if isRunningSleep(row)}<button class="primary" onclick={() => stopSleep(row)}>{c.stopSleep}</button>{/if}
      {#each actions as [action, label]}
        <button class="entry-edit" onclick={() => edit(row, action)}>{label}</button>
      {/each}
      <button class="text-action danger" onclick={() => remove(row)}>{c.deleteEntry}</button>
    </div>
  {/if}
</div>
