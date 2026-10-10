<script>
  import { onMount } from 'svelte';
  import { copy as c } from '../strings.js';
  import Icon from './Icon.svelte';
  import { bottleContents, category, clock, clockTime, compactDateTime, diaperKinds, entrySummary, iconFor,
    isRunningSleep, kindLabels, sleepPlaces } from '../presentation.js';

  // One entry's details. Optional details save as soon as they are picked;
  // larger corrections open their focused editors.
  let { row, nowMs, busy = false, save, edit, stopSleep, remove, close } = $props();

  let noteOpen = $state(false);
  let noteDraft = $state('');
  let heading = $state(null);
  onMount(() => heading?.focus());

  const placeIcons = { 1: 'crib', 2: 'pram', 3: 'contact', 4: 'car', 5: 'dots' };
  const diaperIcons = { 1: 'wet', 2: 'dirty', 3: 'both', 4: 'dry' };
  const target = $derived({ child: row.childId, target: row.id });
  const running = $derived(isRunningSleep(row));
  const instant = ['note', 'feed.bottle', 'feed.solids', 'diaper', 'growth', 'medication', 'temperature'];
  // Corrections that need their own form; inline details are handled above them.
  const changes = $derived([
    !running && ['sleep', 'pump'].includes(row.kind) && row.endMs > row.startMs && ['move', c.moveSession],
    instant.includes(row.kind) && ['time', c.editTime],
    row.kind === 'sleep' && !running && ['sleep-end', c.editSleepDuration],
    row.kind === 'feed.bottle' && row.bottleMl != null && ['bottle', c.editBottle],
    row.kind === 'feed.breast' && row.breastSegments?.length && ['breast', c.editBreast],
    row.kind === 'feed.solids' && ['solids', c.editSolids],
    row.kind === 'pump' && ['pump', c.editPumpAmounts],
    row.kind === 'medication' && row.medicationName && ['medication', c.editMedication],
    row.kind === 'growth' && ['growth', c.editGrowth],
    row.kind === 'temperature' && row.temperatureC != null && ['temperature', c.editTemperature],
  ].filter(Boolean));

  const setPlace = (place) => save({ type: 'editSleepPlace', ...target, place: row.sleepPlace === place ? null : place });
  const setDiaper = (kind) => kind !== row.diaperKind && save({ type: 'editDiaper', ...target, kind });
  const setContent = (content) => content !== row.bottleContent && save({ type: 'editBottle', ...target,
    entered: row.bottleEntered ?? String(row.bottleMl), unit: row.bottleUnit ?? 1, content });
  async function saveNote(event) {
    event.preventDefault();
    if (await save({ type: 'editNote', ...target, text: noteDraft.trim() })) noteOpen = false;
  }
  const onKey = (event) => { if (event.key === 'Escape') close(); };
</script>

<svelte:window onkeydown={onKey} />
<button class="sheet-backdrop" aria-label={c.close} tabindex="-1" onclick={close}></button>
<div class="sheet" role="dialog" aria-modal="true" aria-labelledby="sheet-title">
  <div class="sheet-head {category(row.kind)}">
    <Icon name={iconFor(row.kind)} size={26} />
    <div><h2 id="sheet-title" tabindex="-1" bind:this={heading}>{kindLabels[row.kind] || c.newerEntry}</h2>
      <small>{entrySummary(row)} · {compactDateTime(row.startMs, nowMs)}</small></div>
    <button class="icon-button" aria-label={c.close} onclick={close}><Icon name="close" size={22} /></button>
  </div>

  {#if running}
    <div class="sheet-now"><small>{c.sleepingSince(clockTime(row.startMs))}</small><strong>{clock(nowMs - row.startMs)}</strong></div>
    <button class="primary" disabled={busy} onclick={() => stopSleep(row)}>{c.stopSleep}</button>
  {/if}

  <h3 class="sheet-section">{c.details}<small>{c.optional}</small></h3>
  {#if row.kind === 'sleep'}
    <div class="icon-choices" role="group" aria-label={c.sleepPlace}>
      {#each Object.entries(sleepPlaces) as [code, name]}
        <button aria-pressed={row.sleepPlace === Number(code)} disabled={busy} onclick={() => setPlace(Number(code))}>
          <Icon name={placeIcons[code]} size={28} /><span>{code === '5' ? c.otherShort : name}</span></button>
      {/each}
    </div>
  {:else if row.kind === 'diaper'}
    <div class="icon-choices" role="group" aria-label={c.type}>
      {#each Object.entries(diaperKinds) as [code, name]}
        <button aria-pressed={row.diaperKind === Number(code)} disabled={busy} onclick={() => setDiaper(Number(code))}>
          <Icon name={diaperIcons[code]} size={28} /><span>{name}</span></button>
      {/each}
    </div>
  {:else if row.kind === 'feed.bottle' && (row.bottleEntered != null || row.bottleMl != null)}
    <div class="choices" role="group" aria-label={c.inBottle}>
      {#each Object.entries(bottleContents) as [code, name]}
        <button class="chip" aria-pressed={row.bottleContent === Number(code)} disabled={busy} onclick={() => setContent(Number(code))}>{name}</button>
      {/each}
    </div>
  {/if}

  {#if noteOpen}
    <form class="sheet-note" onsubmit={saveNote}>
      <label>{c.noteText}<textarea rows="3" maxlength="4096" bind:value={noteDraft}></textarea></label>
      {#if row.kind !== 'note'}<p class="muted">{c.clearNoteHint}</p>{/if}
      <div class="form-actions"><button class="primary" type="submit" disabled={busy || (row.kind === 'note' && !noteDraft.trim())}>{c.saveNote}</button>
        <button type="button" class="text-action" onclick={() => noteOpen = false}>{c.cancel}</button></div>
    </form>
  {:else if row.note && row.kind !== 'note'}
    <button class="sheet-row" onclick={() => { noteDraft = row.note; noteOpen = true; }}><span>{c.noteDetail(row.note)}</span><em>{c.edit}</em></button>
  {:else}
    <button class="sheet-row" onclick={() => { noteDraft = row.note ?? ''; noteOpen = true; }}>
      <span>{row.kind === 'note' ? c.editNote : c.addNote}</span><em>＋</em></button>
  {/if}

  {#if changes.length}
    <h3 class="sheet-section">{c.change}</h3>
    {#each changes as [action, label]}
      <button class="sheet-row" onclick={() => edit(row, action)}><span>{label}</span><Icon name="chevron" size={18} /></button>
    {/each}
  {/if}
  <button class="text-action danger sheet-delete" disabled={busy} onclick={() => remove(row)}>{c.deleteEntry}</button>
</div>
