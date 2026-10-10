<script>
  import { copy as c, ageLabel } from '../strings.js';
  import { dateInput, epochDay, sexes } from '../presentation.js';

  // Create a child, or correct an existing child's name, birthday, and sex.
  let { child = null, save, cancel } = $props();
  let name = $state(child?.name ?? '');
  let birthDate = $state(dateInput(child?.birthDay));
  let sex = $state(child?.sex ?? 3);
  let saving = $state(false);

  async function submit(event) {
    event.preventDefault();
    if (saving || !name.trim()) return;
    saving = true;
    try {
      const birthDay = epochDay(birthDate);
      if (!child) {
        await save([{ type: 'child', name: name.trim(), birthDay, sex }]);
        return;
      }
      const actions = [];
      if (name.trim() !== child.name) actions.push({ type: 'renameChild', target: child.id, name: name.trim() });
      if (birthDay !== (child.birthDay ?? null) || sex !== (child.sex ?? null)) {
        actions.push({ type: 'childMetadata', target: child.id, birthDay, sex });
      }
      if (actions.length) await save(actions);
      else cancel();
    } finally { saving = false; }
  }
</script>

<form class="capture-form" onsubmit={submit}>
  <p class="muted">{ageLabel(epochDay(birthDate))}</p>
  <label>{c.childName}<input required maxlength="160" bind:value={name} autocomplete="off" /></label>
  <label>{c.birthDate}<input type="date" max={new Date().toISOString().slice(0, 10)} bind:value={birthDate} /></label>
  {#if birthDate && (!child || child.birthDay == null)}<button type="button" class="text-action" onclick={() => birthDate = ''}>{c.clearBirthDate}</button>{/if}
  <fieldset class="choices"><legend>{c.growthSex}</legend>
    {#each Object.entries(sexes) as [code, text]}
      <button type="button" class="chip" aria-pressed={sex === Number(code)} onclick={() => sex = Number(code)}>{text}</button>
    {/each}
  </fieldset>
  <button class="primary save" type="submit" disabled={!name.trim() || saving}>{child ? c.saveChanges : c.addChild}</button>
</form>
