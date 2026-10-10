<script>
  import { copy as c } from '../strings.js';
  import * as api from '../core.js';
  import { compactDateTime } from '../presentation.js';

  // Save, export, and restore files. Restoring always makes a new local
  // Family; a file never reinstates shared access.
  let { family = '', shared = false, backupCursor = 0, backupGap = false, restore } = $props();

  let protect = $state(false);
  let password = $state('');
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let pending = $state(null);
  let restorePassword = $state('');
  let preview = $state(null);
  let saved = $state(null);
  let revision = $state(null);

  const saveKey = $derived(`babytrack-backup-save-v1:${family}`);
  $effect(() => {
    saved = family ? JSON.parse(localStorage.getItem(saveKey) || 'null') : null;
    if (family) api.localRevision(family).then((value) => revision = value, () => revision = null);
  });

  function download(bytes, name, type) {
    const url = URL.createObjectURL(new Blob([bytes], { type }));
    const link = document.createElement('a');
    link.href = url;
    link.download = name;
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }

  async function work(task) {
    if (busy) return;
    busy = true;
    error = '';
    notice = '';
    try { await task(); }
    catch (cause) { error = cause?.message === 'protected-failure' ? c.wrongPassword : cause?.message || String(cause); }
    finally { busy = false; }
  }

  const saveBackup = () => work(async () => {
    const readable = await api.exportFamily(family);
    if (protect) {
      download(await api.protectBackup(readable, password), 'babytrack-backup.btbk', 'application/octet-stream');
      password = '';
    } else download(readable, 'babytrack-backup.jsonl', 'application/x-ndjson');
    const current = await api.localRevision(family);
    if (current != null) {
      saved = { atMs: Date.now(), revision: current };
      localStorage.setItem(saveKey, JSON.stringify(saved));
      revision = current;
    }
    notice = c.backupSaved;
  });

  const exportCsv = () => work(async () => {
    download(await api.analysisCsv(family), 'babytrack-analysis.csv', 'text/csv');
    notice = c.csvSaved;
  });

  async function choose(file) {
    preview = null;
    pending = null;
    restorePassword = '';
    if (!file) return;
    await work(async () => {
      if (file.size > 64 * 1024 * 1024) throw new Error(c.backupTooLarge);
      const bytes = new Uint8Array(await file.arrayBuffer());
      if (api.isProtectedBackup(bytes)) { pending = bytes; return; }
      preview = { readable: bytes, ...await api.inspectBackup(bytes).catch(() => { throw new Error(c.backupDamaged); }) };
    });
  }

  const checkProtected = () => work(async () => {
    const readable = await api.openProtectedBackup(pending, restorePassword);
    preview = { readable, ...await api.inspectBackup(readable) };
    pending = null;
    restorePassword = '';
  });

  const restoreNow = () => work(async () => {
    const readable = preview.readable;
    preview = null;
    await restore(readable);
  });
</script>

{#if error}<div class="error" role="alert">{error}</div>{/if}
{#if notice}<div class="notice" role="status">{notice}</div>{/if}
{#if family}
  {#if shared}<p class="muted">{c.backupPoint(backupCursor)} {backupGap ? c.backupIncomplete : c.backupComplete}</p>
  {:else if saved}
    <p class="muted">{c.lastFileSave(compactDateTime(saved.atMs))}</p>
    {#if revision != null && revision > saved.revision}<p class="muted">{c.changesSinceSave}</p>{/if}
  {/if}
  <p class="muted">{protect ? c.protectedDescription : c.backupDescription}</p>
  <button type="button" class="chip" aria-pressed={protect} onclick={() => protect = !protect}>{c.protectWithPassword}</button>
  {#if protect}<label>{c.backupPassword}<input type="password" autocomplete="new-password" bind:value={password} /></label>{/if}
  <div class="form-actions">
    <button class="secondary" disabled={busy || (protect && !password)} onclick={saveBackup}>{c.saveBackup}</button>
    <button class="secondary" disabled={busy} onclick={exportCsv}>{c.exportCsv}</button>
  </div>
{/if}
<label>{c.restoreBackup}<input type="file" accept=".jsonl,.json,.btbk" onchange={(event) => {
  const file = event.currentTarget.files?.[0];
  event.currentTarget.value = '';
  choose(file);
}} /></label>
{#if pending}
  <form class="capture-form" onsubmit={(event) => { event.preventDefault(); checkProtected(); }}>
    <p class="muted">{c.enterBackupPassword}</p>
    <label>{c.protectedPassword}<input type="password" autocomplete="current-password" bind:value={restorePassword} /></label>
    <button class="secondary" type="submit" disabled={busy || !restorePassword}>{c.checkProtected}</button>
  </form>
{/if}
{#if preview}
  <div class="panel restore-preview" role="status">
    <p>{c.filePreview(compactDateTime(preview.snapshotMs), preview.records)}</p>
    {#if preview.knownGap}<p class="muted">{c.fileKnownGap}</p>{/if}
    <button class="primary" disabled={busy} onclick={restoreNow}>{c.restoreAsNew}</button>
  </div>
{/if}
{#if busy}<p class="muted" role="status">{c.working}</p>{/if}
