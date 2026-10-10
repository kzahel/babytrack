<script>
  import { copy as c } from '../strings.js';
  import TimeRow from './TimeRow.svelte';
  import { readDraft, persistDraft, tapSide, completedSegments, sideTotals,
    durationLabel as timerLabel } from '../breast-timer.js';
  import { latest, plannedSegments } from '../presentation.js';

  // The timer is a target-scoped browser draft until the core accepts it.
  let { family, child, entries = [], nowMs, save, done } = $props();

  let mode = $state('timer');
  let draft = $state(null);
  let error = $state('');
  let saving = $state(false);
  let logAt = $state(null);
  let side = $state(1);
  let sideMinutes = $state('');
  let sides = $state([]);
  $effect(() => { draft = readDraft(family, child); });

  const lastFeed = $derived(latest(entries, (kind) => kind === 'feed.breast'));
  const lastSide = $derived(lastFeed?.breastSegments?.at(-1)?.side ?? lastFeed?.breastSide ?? null);
  const totals = $derived(sideTotals(draft, nowMs));
  const minutesOk = (value) => /^\d{1,3}$/.test(value) && Number(value) >= 1 && Number(value) <= 240;
  const total = $derived(sides.reduce((sum, item) => sum + item.minutes, 0));
  const canAdd = $derived(minutesOk(sideMinutes) && sides.length < 7 && total + Number(sideMinutes) <= 240);
  const canSaveMinutes = $derived(minutesOk(sideMinutes) && sides.length < 8 && total + Number(sideMinutes) <= 240);

  const timerError = (cause) => ({
    'too-short': c.timerTooShort, 'duration-limit': c.timerTooLong,
    'segment-limit': c.timerTooManySegments, 'empty-timer': c.timerEmpty,
  })[cause?.message] || cause?.message || String(cause);

  function tap(value) {
    error = '';
    try {
      const next = tapSide(draft, value);
      persistDraft(family, child, next);
      draft = next;
    } catch (cause) { error = timerError(cause); }
  }

  async function submit(segments, clear) {
    if (saving) return;
    saving = true;
    error = '';
    try {
      if (await save({ type: 'breast', child, segments })) { clear(); done(); }
    } catch (cause) { error = timerError(cause); }
    finally { saving = false; }
  }

  function saveTimer() {
    try {
      submit(completedSegments(draft), () => { persistDraft(family, child, null); draft = null; });
    } catch (cause) { error = timerError(cause); }
  }

  function saveMinutes() {
    const all = [...sides, { side, minutes: Number(sideMinutes) }];
    submit(plannedSegments(all, logAt ?? Date.now()), () => { sides = []; sideMinutes = ''; });
  }

  function discard() {
    if (!window.confirm(c.discardBreastConfirm)) return;
    persistDraft(family, child, null);
    draft = null;
  }

  const sideName = (value) => (value === 1 ? c.leftBreast : c.rightBreast);
</script>

<div class="segmented" role="group" aria-label={c.breastFeed}>
  <button type="button" aria-pressed={mode === 'timer'} onclick={() => mode = 'timer'}>{c.timerMode}</button>
  <button type="button" aria-pressed={mode === 'minutes'} onclick={() => mode = 'minutes'}>{c.minutesMode}</button>
</div>
{#if error}<div class="error" role="alert">{error}</div>{/if}
{#if mode === 'timer'}
  <p class="muted">{draft?.active ? c.feedingNow : draft?.segments.length ? c.feedingPaused : c.tapSideToStart}</p>
  <div class="breast-totals panel">
    <div><span>{c.leftBreast}</span><strong>{timerLabel(totals[1])}</strong></div>
    <div><span>{c.rightBreast}</span><strong>{timerLabel(totals[2])}</strong></div>
  </div>
  <div class="breast-controls">
    {#each [1, 2] as value}
      <button class:running={draft?.active?.side === value} aria-pressed={draft?.active?.side === value} onclick={() => tap(value)}>
        <span class="breast-control-symbol" aria-hidden="true">{draft?.active?.side === value ? 'Ⅱ' : '▶'}</span>
        <strong>{sideName(value)}</strong>
        <small>{draft?.active?.side === value ? c.tapToPause : !draft && lastSide === value ? c.lastSide : c.tapToStart}</small>
      </button>
    {/each}
  </div>
  <p class="muted">{c.breastTimerHint}</p>
  {#if lastSide}<p class="muted">{c.lastFeedEnded(sideName(lastSide))}</p>{/if}
  {#if draft?.segments.length}
    <div class="panel segment-list"><h2>{c.completedSides}</h2>
      {#each draft.segments as segment}
        <div>{sideName(segment.side)} · {timerLabel(segment.end_utc_ms - segment.start_utc_ms)}</div>
      {/each}
    </div>
  {/if}
  <button class="primary save" disabled={!draft || saving} onclick={saveTimer}>{c.saveBreast}</button>
  {#if draft}<button class="text-action discard" onclick={discard}>{c.discardSession}</button>{/if}
{:else}
  <form class="capture-form" onsubmit={(event) => { event.preventDefault(); if (canSaveMinutes) saveMinutes(); }}>
    <TimeRow bind:value={logAt} label={c.finishedAt} />
    <fieldset class="choices"><legend>{c.sideLabel}</legend>
      {#each [1, 2] as value}
        <button type="button" class="chip" aria-pressed={side === value} onclick={() => side = value}>{value === 1 ? c.left : c.right}</button>
      {/each}
    </fieldset>
    <label>{c.minutesOnSide}<input inputmode="numeric" maxlength="3" bind:value={sideMinutes} /></label>
    {#if sides.length}<p class="muted">{c.segmentsDraft(sides.map((item) => `${item.side === 1 ? c.left : c.right} ${c.minutesShort(item.minutes)}`).join(' → '))}</p>{/if}
    <div class="form-actions">
      <button type="button" class="secondary" disabled={!canAdd} onclick={() => {
        sides = [...sides, { side, minutes: Number(sideMinutes) }];
        side = side === 1 ? 2 : 1;
        sideMinutes = '';
      }}>{c.addSegment}</button>
      {#if sides.length}<button type="button" class="text-action" onclick={() => sides = sides.slice(0, -1)}>{c.removeSegment}</button>{/if}
    </div>
    <button class="primary save" type="submit" disabled={!canSaveMinutes || saving}>{c.saveBreastFeed}</button>
  </form>
{/if}
