<script>
  import { copy as c } from '../strings.js';

  // Verified devices for a shared Family. Names are labels on this browser only.
  let { family, devices = [], deviceId = '', makeCopy } = $props();
  let open = $state(false);
  let naming = $state('');
  let draft = $state('');
  let labels = $state({});

  const key = $derived(`babytrack-device-labels-v1:${family}`);
  $effect(() => { labels = JSON.parse(localStorage.getItem(key) || '{}'); });

  function saveLabel(id) {
    const next = { ...labels };
    if (draft.trim()) next[id] = draft.trim().slice(0, 40);
    else delete next[id];
    labels = next;
    localStorage.setItem(key, JSON.stringify(next));
    naming = '';
  }
  const describe = (device) => c.entry(labels[device.deviceId],
    device.deviceId === deviceId ? c.thisDevice : c.otherDevice,
    device.role === 2 ? c.manager : c.member, c.deviceId(device.deviceId));
</script>

<p>{c.devicesWithAccess(devices.length)}</p>
<button type="button" class="text-action" aria-expanded={open} onclick={() => open = !open}>{open ? c.hideFamilyAccess : c.familyAccess}</button>
{#if open}
  <h3>{c.verifiedDevices}</h3>
  {#each devices as device (device.deviceId)}
    <div class="device-row">
      <span>{describe(device)}</span>
      {#if naming === device.deviceId}
        <form class="device-name" onsubmit={(event) => { event.preventDefault(); saveLabel(device.deviceId); }}>
          <label>{c.deviceName}<input maxlength="40" bind:value={draft} /></label>
          <button class="secondary" type="submit">{c.save}</button>
        </form>
      {:else}
        <button type="button" class="text-action" onclick={() => { naming = device.deviceId; draft = labels[device.deviceId] ?? ''; }}>{c.nameDevice}</button>
      {/if}
    </div>
  {/each}
  <p class="muted">{c.managerActionsAndroid}</p>
  <button type="button" class="secondary" onclick={makeCopy}>{c.makeFamilyCopy}</button>
{/if}
