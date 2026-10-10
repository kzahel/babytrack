<script>
  import { copy as c, ageLabel } from '../strings.js';

  // The selected child with a switcher, age, date, and a quiet sync status.
  let { child, children = [], familyLabel = '', shared = false, syncStage = '', selectChild } = $props();
  const today = new Intl.DateTimeFormat(undefined, { weekday: 'short', month: 'short', day: 'numeric' }).format(Date.now());
  const status = $derived(!shared ? { tone: 'local', text: c.onThisDevice }
    : syncStage === c.syncFailed ? { tone: 'warn', text: c.syncFailed }
    : syncStage === c.savedPending ? { tone: 'pending', text: c.savedPending }
    : syncStage && syncStage !== c.syncReady ? { tone: 'pending', text: syncStage }
    : { tone: 'ok', text: c.sharedSynced });
</script>

<div class="app-header">
  <span class="avatar" aria-hidden="true">{child?.name.slice(0, 1).toUpperCase() || '·'}</span>
  <div class="who">
    <div class="who-name">
      <h1>{child?.name || c.chooseChild}</h1>
      {#if children.length > 1}
        <span class="who-caret" aria-hidden="true">▾</span>
        <select aria-label={c.chooseChild} value={child?.id} onchange={(event) => selectChild(event.currentTarget.value)}>
          {#each children as row}<option value={row.id}>{row.name}</option>{/each}
        </select>
      {/if}
    </div>
    <small>{[child ? ageLabel(child.birthDay) : '', today, familyLabel].filter(Boolean).join(' · ')}</small>
  </div>
  <span class="sync {status.tone}" role="status"><i></i>{status.text}</span>
</div>
