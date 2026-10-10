<script>
  import { untrack } from 'svelte';
  import { copy as c } from '../strings.js';
  import { bottleContents, bottleUnits, canonicalDecimal, diaperKinds, fromLocalInput, lengthUnits,
    localInput, localized, sleepPlaces, temperatureUnits, validDecimal, weightUnits } from '../presentation.js';
  import { editableSegments, rebuiltSegments } from '../breast-timer.js';

  // One correction of one saved entry. The core binds it to the same record.
  // The parent remounts this form per entry, so fields start from that row.
  let { row: entry, child: entryChild, action, save, done } = $props();
  const row = untrack(() => entry);
  const child = untrack(() => entryChild);

  let saving = $state(false);
  let error = $state('');
  let text = $state(row.note ?? '');
  let when = $state(localInput(row.startMs));
  let sleepMinutes = $state(String(Math.round(((row.endMs ?? row.startMs) - row.startMs) / 60_000)));
  let place = $state(row.sleepPlace ?? null);
  let bottleAmount = $state(localized(row.bottleEntered ?? row.bottleMl ?? ''));
  let bottleUnit = $state(row.bottleUnit ?? 1);
  let bottleContent = $state(row.bottleContent ?? 4);
  let diaperKind = $state(row.diaperKind);
  let foods = $state((row.solidsFoods || []).join('\n'));
  let solidsAmount = $state(row.solidsAmount ?? '');
  let pumpLeft = $state(row.pumpLeftMl != null ? String(row.pumpLeftMl) : '');
  let pumpRight = $state(row.pumpRightMl != null ? String(row.pumpRightMl) : '');
  let pumpTotal = $state(row.pumpTotalMl != null ? String(row.pumpTotalMl) : '');
  let medicationName = $state(row.medicationName ?? '');
  let doseAmount = $state(row.medicationDoseAmount ?? '');
  let doseUnit = $state(row.medicationDoseUnit ?? '');
  let temperature = $state(localized(row.temperatureEntered ?? row.temperatureC ?? ''));
  let temperatureUnit = $state(row.temperatureEntered != null ? row.temperatureUnit ?? 30 : 30);
  let growth = $state({
    weight: { value: '', unit: row.growthWeightUnit ?? 10 },
    length: { value: '', unit: row.growthLengthUnit ?? 20 },
    head: { value: '', unit: row.growthHeadUnit ?? 20 },
  });
  let segments = $state(row.breastSegments ? editableSegments(row.breastSegments) : []);
  let breastStart = $state(localInput(row.startMs));
  let keep = $state('finish');

  const base = { child, target: row.id };
  const offsetAt = (ms) => -new Date(ms).getTimezoneOffset();
  const optional = (value) => (String(value).trim() ? Number(value) : null);
  const optionalMl = (value) => !value.trim() || (/^\d+$/.test(value.trim()) && Number(value) <= 1_000_000);
  const growthFields = [['weight', c.weight, weightUnits], ['length', c.length, lengthUnits],
    ['head', c.headCircumference, lengthUnits]];

  function past(ms) {
    if (!Number.isFinite(ms) || ms > Date.now()) throw new Error(c.futureTime);
    return ms;
  }

  // Build the core action for this correction, or throw a person-facing error.
  function build() {
    switch (action) {
      case 'note': return { type: 'editNote', ...base, text: text.trim() };
      case 'time': {
        const start = past(fromLocalInput(when));
        return { type: 'editTime', ...base, startMs: start, offset: offsetAt(start) };
      }
      case 'move': {
        const start = past(fromLocalInput(when));
        const end = past(start + row.endMs - row.startMs);
        return { type: 'moveInterval', ...base, startMs: start, offset: offsetAt(start), endMs: end,
          endOffset: offsetAt(end) };
      }
      case 'sleep-end': {
        const minutes = Number(sleepMinutes);
        if (!/^\d{1,4}$/.test(sleepMinutes) || minutes < 1 || minutes > 1440) throw new Error(c.sleepMinutesRule);
        const end = past(row.startMs + minutes * 60_000);
        return { type: 'editSleepEnd', ...base, endMs: end, endOffset: offsetAt(end) };
      }
      case 'sleep-place': return { type: 'editSleepPlace', ...base, place };
      case 'bottle':
        if (!validDecimal(bottleAmount, { integer: bottleUnit === 1, max: bottleUnit === 1 ? 1_000_000 : undefined })) {
          throw new Error(c.amountRule);
        }
        return { type: 'editBottle', ...base, entered: canonicalDecimal(bottleAmount), unit: bottleUnit,
          content: bottleContent };
      case 'diaper': return { type: 'editDiaper', ...base, kind: diaperKind };
      case 'solids': return { type: 'editSolids', ...base, amount: solidsAmount,
        foods: foods.split('\n').map((line) => line.trim()).filter(Boolean) };
      case 'pump':
        if (![pumpLeft, pumpRight, pumpTotal].every(optionalMl) || (pumpTotal.trim()
          ? pumpLeft.trim() || pumpRight.trim() || Number(pumpTotal) < 1
          : Number(pumpLeft || 0) + Number(pumpRight || 0) < 1)) throw new Error(c.pumpRule);
        return { type: 'editPump', ...base, leftMl: optional(pumpLeft), rightMl: optional(pumpRight),
          totalMl: optional(pumpTotal) };
      case 'medication': return { type: 'editMedication', ...base, name: medicationName.trim(),
        doseAmount: doseAmount.trim(), doseUnit: doseUnit.trim() };
      case 'growth': {
        const entered = Object.fromEntries(growthFields.map(([field]) => {
          const { value, unit } = growth[field];
          if (value.trim() && !validDecimal(value, { integer: unit === 10 || unit === 20 })) throw new Error(c.amountRule);
          return [field, value.trim() ? { entered: canonicalDecimal(value), unit } : null];
        }));
        if (!Object.values(entered).some(Boolean)) throw new Error(c.growthRule);
        return { type: 'editGrowth', ...base, ...entered };
      }
      case 'temperature':
        if (!validDecimal(temperature, { signed: true })) throw new Error(c.amountRule);
        return { type: 'editTemperature', ...base, entered: canonicalDecimal(temperature), unit: temperatureUnit };
      case 'breast': {
        const start = fromLocalInput(breastStart);
        const moved = start !== fromLocalInput(localInput(row.startMs));
        return { type: 'editBreast', ...base, segments: rebuiltSegments(row.breastSegments, segments,
          { startMs: moved ? past(start) : null, keep }) };
      }
      default: throw new Error(c.saveFailed);
    }
  }

  const timerErrors = { 'invalid-duration': c.invalidDuration, 'future-end': c.futureEnd,
    'duration-limit': c.timerTooLong, 'segment-limit': c.timerTooManySegments };

  async function submit(event) {
    event.preventDefault();
    if (saving) return;
    error = '';
    let edit;
    try { edit = build(); }
    catch (cause) { error = timerErrors[cause.message] || cause.message; return; }
    saving = true;
    try { if (await save(edit)) done(); }
    finally { saving = false; }
  }
</script>

{#snippet choices(label, options, current, choose)}
  <fieldset class="choices"><legend>{label}</legend>
    {#each Object.entries(options) as [code, name]}
      <button type="button" class="chip" aria-pressed={current === Number(code)} onclick={() => choose(Number(code))}>{name}</button>
    {/each}
  </fieldset>
{/snippet}

<form class="capture-form" onsubmit={submit}>
  {#if error}<div class="error" role="alert">{error}</div>{/if}
  {#if action === 'note'}
    <label>{c.noteText}<textarea rows="5" maxlength="4096" bind:value={text}></textarea></label>
    {#if row.kind !== 'note'}<p class="muted">{c.clearNoteHint}</p>{/if}
  {:else if action === 'time' || action === 'move'}
    <label>{action === 'move' ? c.moveStart : c.whenLabel}<input type="datetime-local" max={localInput(Date.now())} bind:value={when} /></label>
    {#if action === 'move'}<p class="muted">{c.moveHint}</p>{/if}
  {:else if action === 'sleep-end'}
    <label>{c.minutesSlept}<input inputmode="numeric" maxlength="4" bind:value={sleepMinutes} /></label>
  {:else if action === 'sleep-place'}
    <fieldset class="choices"><legend>{c.sleepPlace}</legend>
      <button type="button" class="chip" aria-pressed={place == null} onclick={() => place = null}>{c.placeNone}</button>
      {#each Object.entries(sleepPlaces) as [code, name]}
        <button type="button" class="chip" aria-pressed={place === Number(code)} onclick={() => place = Number(code)}>{name}</button>
      {/each}
    </fieldset>
  {:else if action === 'bottle'}
    <label>{c.bottleAmount} ({bottleUnits[bottleUnit]})<input inputmode={bottleUnit === 1 ? 'numeric' : 'decimal'} bind:value={bottleAmount} /></label>
    {@render choices(c.unit, bottleUnits, bottleUnit, (code) => bottleUnit = code)}
    {@render choices(c.inBottle, bottleContents, bottleContent, (code) => bottleContent = code)}
  {:else if action === 'diaper'}
    <div class="tile-choices">
      {#each Object.entries(diaperKinds) as [code, name]}
        <button type="button" aria-pressed={diaperKind === Number(code)} onclick={() => diaperKind = Number(code)}>{name}</button>
      {/each}
    </div>
  {:else if action === 'solids'}
    <label>{c.foods}<textarea rows="4" maxlength="2048" bind:value={foods}></textarea></label>
    <label>{c.amountEaten}<input maxlength="256" bind:value={solidsAmount} /></label>
  {:else if action === 'pump'}
    <label>{c.leftMlLabel}<input inputmode="numeric" maxlength="6" bind:value={pumpLeft} /></label>
    <label>{c.rightMlLabel}<input inputmode="numeric" maxlength="6" bind:value={pumpRight} /></label>
    <label>{c.totalMlLabel}<input inputmode="numeric" maxlength="6" bind:value={pumpTotal} /></label>
    <p class="muted">{c.pumpEditHint}</p>
  {:else if action === 'medication'}
    <label>{c.medicationName}<input maxlength="256" required bind:value={medicationName} /></label>
    <label>{c.doseAmount}<input maxlength="64" required bind:value={doseAmount} /></label>
    <label>{c.doseUnit}<input maxlength="64" required bind:value={doseUnit} /></label>
  {:else if action === 'growth'}
    {#each growthFields as [field, label, units]}
      <div class="measure">
        <label>{label}<input inputmode="decimal" maxlength="16" bind:value={growth[field].value} /></label>
        {@render choices(`${label} · ${c.unit}`, units, growth[field].unit, (code) => { growth[field].unit = code; growth[field].value = ''; })}
      </div>
    {/each}
    <p class="muted">{c.growthEditHint}</p>
  {:else if action === 'temperature'}
    {@render choices(c.unit, temperatureUnits, temperatureUnit, (code) => { temperatureUnit = code; temperature = ''; })}
    <label>{c.temperatureIn(temperatureUnits[temperatureUnit])}<input inputmode="decimal" maxlength="16" bind:value={temperature} /></label>
  {:else if action === 'breast'}
    <label>{c.breastStart}<input type="datetime-local" max={localInput(Date.now())} bind:value={breastStart} /></label>
    <fieldset class="choices"><legend>{c.whenChangingMinutes}</legend>
      <button type="button" class="chip" aria-pressed={keep === 'finish'} onclick={() => keep = 'finish'}>{c.keepFinish}</button>
      <button type="button" class="chip" aria-pressed={keep === 'start'} onclick={() => keep = 'start'}>{c.keepStart}</button>
    </fieldset>
    {#each segments as segment, index}
      <div class="panel edit-segment">
        <h2>{c.sideNumber(index + 1)}</h2>
        <fieldset class="choices"><legend>{c.side}</legend>
          {#each ['1', '2'] as value}
            <button type="button" class="chip" aria-pressed={segment.side === value}
              onclick={() => segment.side = value}>{value === '1' ? c.left : c.right}</button>
          {/each}
        </fieldset>
        {#if index > 0}<label>{c.pauseBefore}<input inputmode="numeric" bind:value={segment.pause} required /></label>{/if}
        <label>{c.durationMmSs}<input inputmode="numeric" bind:value={segment.duration} required /></label>
      </div>
    {/each}
    <div class="form-actions">
      {#if segments.length < 8}<button type="button" class="secondary" onclick={() => segments = [...segments,
        { side: segments.at(-1)?.side === '1' ? '2' : '1', pause: '0:00', duration: '5:00' }]}>{c.addSegment}</button>{/if}
      {#if segments.length > 1}<button type="button" class="text-action" onclick={() => segments = segments.slice(0, -1)}>{c.removeSegment}</button>{/if}
    </div>
  {/if}
  <button class="primary save" type="submit" disabled={saving}>{c.saveChanges}</button>
</form>
