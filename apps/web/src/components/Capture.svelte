<script>
  import { copy as c } from '../strings.js';
  import TimeRow from './TimeRow.svelte';
  import { bottleContents, bottleUnits, canonicalDecimal, clock, diaperKinds, latest, lengthUnits, localized, sleepPlaces, temperatureUnits, validDecimal,
    weightUnits } from '../presentation.js';

  // One focused form per activity. `save(action)` resolves true once the core
  // accepted the entry; drafts here are UI-only until then.
  let { kind, family, child, entries = [], save, done, startSleep, nowMs } = $props();

  let logAt = $state(null);
  let saving = $state(false);
  let diaperKind = $state(null);
  let bottleContent = $state(2);
  let bottleUnit = $state(1);
  let bottleAmount = $state('');
  let pumpMinutes = $state('');
  let pumpLeft = $state('');
  let pumpRight = $state('');
  let pumpTotal = $state('');
  let foods = $state('');
  let solidsAmount = $state('');
  let sleepMinutes = $state('');
  let sleepPlace = $state(null);
  let growth = $state({
    weight: { value: '', unit: 11 }, length: { value: '', unit: 21 }, head: { value: '', unit: 21 },
  });
  const growthFields = [['weight', c.weight, weightUnits], ['length', c.length, lengthUnits],
    ['head', c.headCircumference, lengthUnits]];
  let temperature = $state('');
  let temperatureUnit = $state(30);
  let medicationName = $state('');
  let doseAmount = $state('');
  let doseUnit = $state('');
  let noteText = $state('');

  const pumpKey = $derived(`babytrack-pump-draft-v1:${family}:${child}`);
  let pumpStart = $state(null);
  $effect(() => { pumpStart = Number(localStorage.getItem(pumpKey)) || null; });

  const lastBottle = $derived(latest(entries.filter((row) => row.bottleEntered != null && row.bottleUnit),
    (value) => value === 'feed.bottle'));
  const integerBottle = $derived(bottleUnit === 1);
  const bottleValid = $derived(validDecimal(bottleAmount, { integer: integerBottle,
    max: integerBottle ? 1_000_000 : undefined }));
  const whole = (value, max) => /^\d+$/.test(String(value).trim()) && Number(value) >= 1 && Number(value) <= max;
  const optionalMl = (value) => String(value).trim() === '' || (/^\d+$/.test(value.trim()) && Number(value) <= 1_000_000);
  const pumpValid = $derived(whole(pumpMinutes, 240) && optionalMl(pumpLeft) && optionalMl(pumpRight) &&
    optionalMl(pumpTotal) && (pumpTotal.trim()
      ? !pumpLeft.trim() && !pumpRight.trim() && Number(pumpTotal) > 0
      : Number(pumpLeft || 0) + Number(pumpRight || 0) > 0));
  const measureValid = ({ value, unit }) => !value.trim() || validDecimal(value, { integer: unit === 10 || unit === 20 });
  const growthValid = $derived(Object.values(growth).some((item) => item.value.trim()) &&
    Object.values(growth).every(measureValid));

  const when = () => {
    const at = logAt ?? Date.now();
    return { child, startMs: at, offset: -new Date(at).getTimezoneOffset() };
  };
  const optional = (value) => (value.trim() ? Number(value) : null);
  const measure = ({ value, unit }) => (value.trim() ? { entered: canonicalDecimal(value), unit } : null);

  async function submit(action) {
    if (saving) return;
    saving = true;
    try {
      if (!await save(action)) return;
      if (kind === 'pump') localStorage.removeItem(pumpKey);
      done();
    } finally { saving = false; }
  }

  function intervalEnding(minutesValue) {
    const end = logAt ?? Date.now();
    return { child, startMs: end - Number(minutesValue) * 60_000,
      offset: -new Date(end - Number(minutesValue) * 60_000).getTimezoneOffset(), endMs: end };
  }

  function step(direction) {
    const size = integerBottle ? 10 : 0.5;
    const next = Math.max(0, (Number(canonicalDecimal(bottleAmount)) || 0) + direction * size);
    bottleAmount = next ? String(next) : '';
  }

  function togglePump() {
    if (pumpStart == null) {
      pumpStart = Date.now();
      localStorage.setItem(pumpKey, String(pumpStart));
      return;
    }
    const stop = Date.now();
    pumpMinutes = String(Math.max(1, Math.round((stop - pumpStart) / 60_000)));
    logAt = stop;
    pumpStart = null;
    localStorage.removeItem(pumpKey);
  }

  function onSubmit(event) {
    event.preventDefault();
    if (kind === 'diaper' && diaperKind) submit({ type: 'diaper', ...when(), kind: diaperKind });
    else if (kind === 'feed.bottle' && bottleValid) {
      submit({ type: 'bottle', ...when(), entered: canonicalDecimal(bottleAmount), unit: bottleUnit,
        content: bottleContent });
    } else if (kind === 'pump' && pumpValid) {
      submit({ type: 'pump', ...intervalEnding(pumpMinutes), leftMl: optional(pumpLeft),
        rightMl: optional(pumpRight), totalMl: optional(pumpTotal) });
    } else if (kind === 'feed.solids' && foods.trim()) {
      submit({ type: 'solids', ...when(), amount: solidsAmount.trim(),
        foods: foods.split('\n').map((line) => line.trim()).filter(Boolean) });
    } else if (kind === 'sleep' && whole(sleepMinutes, 1440)) {
      const interval = intervalEnding(sleepMinutes);
      submit({ type: 'sleep', ...interval, endOffset: -new Date(interval.endMs).getTimezoneOffset(),
        place: sleepPlace });
    } else if (kind === 'growth' && growthValid) {
      submit({ type: 'growth', ...when(), weight: measure(growth.weight),
        length: measure(growth.length), head: measure(growth.head) });
    } else if (kind === 'temperature' && validDecimal(temperature, { signed: true })) {
      submit({ type: 'temperature', ...when(), entered: canonicalDecimal(temperature), unit: temperatureUnit });
    } else if (kind === 'medication' && medicationName.trim() && doseAmount.trim() && doseUnit.trim()) {
      submit({ type: 'medication', ...when(), name: medicationName.trim(), doseAmount: doseAmount.trim(),
        doseUnit: doseUnit.trim() });
    } else if (kind === 'note' && noteText.trim()) submit({ type: 'note', ...when(), text: noteText.trim() });
  }
</script>

{#snippet choices(label, options, current, choose)}
  <fieldset class="choices"><legend>{label}</legend>
    {#each Object.entries(options) as [code, text]}
      <button type="button" class="chip" aria-pressed={current === Number(code)}
        onclick={() => choose(Number(code))}>{text}</button>
    {/each}
  </fieldset>
{/snippet}

<form class="capture-form" onsubmit={onSubmit}>
  <TimeRow bind:value={logAt} />
  {#if kind === 'diaper'}
    <div class="tile-choices">
      {#each Object.entries(diaperKinds) as [code, text]}
        <button type="button" aria-pressed={diaperKind === Number(code)} onclick={() => diaperKind = Number(code)}>{text}</button>
      {/each}
    </div>
    <button class="primary save" type="submit" disabled={!diaperKind || saving}>{c.saveDiaper}</button>
  {:else if kind === 'feed.bottle'}
    {@render choices(c.inBottle, bottleContents, bottleContent, (code) => bottleContent = code)}
    {@render choices(c.unit, bottleUnits, bottleUnit, (code) => { bottleUnit = code; bottleAmount = ''; })}
    <label>{c.bottleAmount} ({bottleUnits[bottleUnit]})
      <input inputmode={integerBottle ? 'numeric' : 'decimal'} bind:value={bottleAmount} autocomplete="off" /></label>
    <div class="stepper">
      <button type="button" class="secondary" onclick={() => step(-1)}>{c.less}</button>
      <button type="button" class="secondary" onclick={() => step(1)}>{c.more}</button>
    </div>
    {#if lastBottle}<button type="button" class="chip" onclick={() => { bottleUnit = lastBottle.bottleUnit; bottleAmount = localized(lastBottle.bottleEntered); }}>
      {c.sameAsLast(localized(lastBottle.bottleEntered), bottleUnits[lastBottle.bottleUnit])}</button>{/if}
    <button class="primary save" type="submit" disabled={!bottleValid || saving}>{c.saveBottle}</button>
  {:else if kind === 'pump'}
    <div class="panel pump-timer">
      {#if pumpStart != null}<span>{c.pumping}</span><strong>{clock(nowMs - pumpStart)}</strong>{/if}
      <button type="button" class="secondary" onclick={togglePump}>{pumpStart == null ? c.startPumpTimer : c.stopTimer}</button>
    </div>
    <label>{c.minutesPumping}<input inputmode="numeric" maxlength="3" bind:value={pumpMinutes} /></label>
    <fieldset class="choices"><legend>{c.amount}</legend>
      <label>{c.leftMlLabel}<input inputmode="numeric" maxlength="6" bind:value={pumpLeft} /></label>
      <label>{c.rightMlLabel}<input inputmode="numeric" maxlength="6" bind:value={pumpRight} /></label>
      <label>{c.totalMlLabel}<input inputmode="numeric" maxlength="6" bind:value={pumpTotal} /></label>
    </fieldset>
    <p class="muted">{c.pumpRule}</p>
    <button class="primary save" type="submit" disabled={!pumpValid || saving}>{c.savePumping}</button>
  {:else if kind === 'feed.solids'}
    <label>{c.foods}<textarea rows="4" maxlength="2048" bind:value={foods}></textarea></label>
    <label>{c.amountEaten}<input maxlength="256" bind:value={solidsAmount} /></label>
    <button class="primary save" type="submit" disabled={!foods.trim() || saving}>{c.saveSolids}</button>
  {:else if kind === 'sleep'}
    <label>{c.minutesSlept}<input inputmode="numeric" maxlength="4" bind:value={sleepMinutes} /></label>
    <fieldset class="choices"><legend>{c.sleepPlace}</legend>
      <button type="button" class="chip" aria-pressed={sleepPlace == null} onclick={() => sleepPlace = null}>{c.placeNone}</button>
      {#each Object.entries(sleepPlaces) as [code, text]}
        <button type="button" class="chip" aria-pressed={sleepPlace === Number(code)} onclick={() => sleepPlace = Number(code)}>{text}</button>
      {/each}
    </fieldset>
    <div class="form-actions">
      <button type="button" class="secondary" disabled={saving} onclick={() => startSleep(sleepPlace)}>{c.startSleep}</button>
      <button class="primary" type="submit" disabled={!whole(sleepMinutes, 1440) || saving}>{c.saveSleep}</button>
    </div>
  {:else if kind === 'growth'}
    {#each growthFields as [field, label, units]}
      <div class="measure">
        <label>{label}<input inputmode="decimal" maxlength="16" bind:value={growth[field].value} /></label>
        {@render choices(`${label} · ${c.unit}`, units, growth[field].unit,
          (code) => { growth[field].unit = code; growth[field].value = ''; })}
      </div>
    {/each}
    <p class="muted">{c.growthRule}</p>
    <button class="primary save" type="submit" disabled={!growthValid || saving}>{c.saveGrowth}</button>
  {:else if kind === 'temperature'}
    {@render choices(c.unit, temperatureUnits, temperatureUnit, (code) => { temperatureUnit = code; temperature = ''; })}
    <label>{c.temperatureIn(temperatureUnits[temperatureUnit])}<input inputmode="decimal" maxlength="16" bind:value={temperature} /></label>
    <button class="primary save" type="submit" disabled={!validDecimal(temperature, { signed: true }) || saving}>{c.saveTemperature}</button>
  {:else if kind === 'medication'}
    <label>{c.medicationName}<input maxlength="256" bind:value={medicationName} /></label>
    <label>{c.doseAmount}<input maxlength="64" bind:value={doseAmount} /></label>
    <label>{c.doseUnit}<input maxlength="64" bind:value={doseUnit} /></label>
    <button class="primary save" type="submit"
      disabled={!medicationName.trim() || !doseAmount.trim() || !doseUnit.trim() || saving}>{c.saveMedication}</button>
  {:else}
    <label>{c.whatHappened}<textarea rows="5" maxlength="4096" bind:value={noteText}></textarea></label>
    <button class="primary save" type="submit" disabled={!noteText.trim() || saving}>{c.saveNote}</button>
  {/if}
</form>
