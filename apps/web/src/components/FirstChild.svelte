<script>
  import { copy as c } from '../strings.js';
  import { epochDay } from '../presentation.js';
  import Icon from './Icon.svelte';

  // The only setup step: a name, an optional birthday, then tracking.
  let { start, back } = $props();
  let name = $state('');
  let birthDate = $state('');
  let showBirth = $state(false);
  let saving = $state(false);

  async function submit(event) {
    event.preventDefault();
    if (saving || !name.trim()) return;
    saving = true;
    try { await start(name.trim(), epochDay(birthDate)); }
    finally { saving = false; }
  }
</script>

<form class="form-view first-child" onsubmit={submit}>
  <button type="button" class="back" onclick={back} disabled={saving}>← {c.back}</button>
  <h1>{c.whatsName}</h1>
  <p class="muted">{c.nicknameHint}</p>
  <label>{c.nameOrNickname}<input required maxlength="160" bind:value={name} autocomplete="off" disabled={saving} /></label>
  {#if showBirth || birthDate}
    <label>{c.birthDateOptional}<input type="date" max={new Date().toISOString().slice(0, 10)} bind:value={birthDate} disabled={saving} /></label>
  {:else}
    <button type="button" class="optional-row" onclick={() => showBirth = true}><span>{c.birthDate}</span><em>{c.addOptional}</em></button>
  {/if}
  <p class="reassure"><Icon name="lock" size={18} />{c.savedOnlyHere}</p>
  <button class="primary save" type="submit" disabled={!name.trim() || saving}>{c.startTracking}</button>
</form>
