<script>
  import { copy as c } from '../strings.js';
  import { compactDateTime, fromLocalInput, localInput } from '../presentation.js';

  // `value` is a chosen past instant in UTC ms, or null for "now".
  let { value = $bindable(null), label = c.whenLabel } = $props();
  let editing = $state(false);
  let error = $state('');

  function choose(text) {
    const ms = fromLocalInput(text);
    if (!Number.isFinite(ms)) return;
    if (ms > Date.now()) { error = c.futureTime; return; }
    error = '';
    value = ms;
  }
</script>

<div class="time-row">
  <div><span>{label}</span><strong>{value == null ? c.now : compactDateTime(value)}</strong></div>
  <button type="button" class="text-action" onclick={() => editing = !editing}>{c.changeTime}</button>
  {#if value != null}<button type="button" class="text-action" onclick={() => { value = null; editing = false; error = ''; }}>{c.useNow}</button>{/if}
  {#if editing}
    <label class="time-input">{label}<input type="datetime-local" max={localInput(Date.now())}
      value={localInput(value ?? Date.now())} onchange={(event) => choose(event.currentTarget.value)} /></label>
  {/if}
  {#if error}<p class="field-error" role="alert">{error}</p>{/if}
</div>
