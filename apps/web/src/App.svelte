<script>
  import { onMount } from 'svelte';
  import { copy as c, ageLabel } from './strings.js';
  import * as api from './core.js';
  import { createTrackerController } from './tracker-controller.js';
  import { readDraft } from './breast-timer.js';
  import { chooser, clock, clockTime, dayWindow, entrySummary, kindLabels, iconFor, category, sleepPlaces } from './presentation.js';
  import Icon from './components/Icon.svelte';
  import Capture from './components/Capture.svelte';
  import BreastFeed from './components/BreastFeed.svelte';
  import ChildProfile from './components/ChildProfile.svelte';
  import EntryEditor from './components/EntryEditor.svelte';
  import Today from './components/Today.svelte';
  import History from './components/History.svelte';
  import BackupPanel from './components/BackupPanel.svelte';
  import EntrySheet from './components/EntrySheet.svelte';
  import SharingPanel from './components/SharingPanel.svelte';
  import Welcome from './components/Welcome.svelte';
  import FirstChild from './components/FirstChild.svelte';
  import AppHeader from './components/AppHeader.svelte';

  let error = '';
  let notice = '';
  let tab = 'today';
  let screen = '';
  let captureKind = '';
  let profileChild = null;
  let breastDraft = null;
  let pumpStartMs = null;
  let sheetId = '';
  let nowMs = Date.now();
  let editing = null;
  let undo = null;
  let invitationInput = '';
  let actionSequence = 0;
  let sleepBusy = false;
  const terminalJoinStages = [c.inviteClaimed, c.inviteCanceled, c.inviteExpired, c.inviteInvalidated];

  $: sharedSelected = $tracker.familyRows.find((row) => row.family === $tracker.family)?.source === 'shared';
  $: selectedChild = data.children.find((row) => row.id === child);
  $: familyLabel = c.familyNumber(familyRows.findIndex((row) => row.family === family) + 1);
  $: entries = data.activities.filter((row) => row.childId === child);
  // The core's totals for today; refreshed on new data and each minute.
  let todaySummary = null;
  let summarySequence = 0;
  $: refreshToday(family, child, data, Math.floor(nowMs / 60_000));
  function refreshToday(targetFamily, targetChild) {
    const request = ++summarySequence;
    if (!targetFamily || !targetChild) { todaySummary = null; return; }
    loadSummary(dayWindow(Date.now())).then((value) => { if (request === summarySequence) todaySummary = value; },
      () => { if (request === summarySequence) todaySummary = null; });
  }
  const loadSummary = (range) => api.daySummary(family, child, range);

  const tracker = createTrackerController({ api, copy: c, preferences: localStorage,
    onSelect: () => { screen = ''; editing = null; sheetId = ''; tab = 'today'; },
    onRemoval: () => { screen = ''; editing = null; },
    onRememberInvitation: () => history.replaceState(null, '', location.pathname + location.search) });
  $: ({ loading, familyRows, family, child, data, pendingFragment, joinStage,
    syncStage, removedInfo, privateCopy, backupGap, backupCursor, devices, deviceId } = $tracker);
  // Reread the browser-local nursing draft whenever a route closes.
  $: breastDraft = screen === '' && $tracker.family && $tracker.child ? readDraft($tracker.family, $tracker.child) : breastDraft;
  $: pumpStartMs = screen === '' && $tracker.family && $tracker.child
    ? Number(localStorage.getItem(`babytrack-pump-draft-v1:${$tracker.family}:${$tracker.child}`)) || null : pumpStartMs;
  // The entry in the details sheet, kept current; the sheet hides if the entry goes away.
  $: sheetRow = sheetId ? entries.find((row) => row.id === sheetId) || null : null;

  onMount(() => {
    const clock = setInterval(() => nowMs = Date.now(), 1000);
    const poller = setInterval(() => { if (document.visibilityState === 'visible') poll(); }, 15000);
    void tracker.initialize(location.hash).catch((cause) => error = message(cause));
    return () => { clearInterval(clock); clearInterval(poller); tracker.dispose(); };
  });

  function message(cause) { return cause?.message || String(cause); }
  async function run(action) {
    const sequence = ++actionSequence;
    error = '';
    notice = '';
    try { await action(); }
    catch (cause) { if (sequence === actionSequence) error = message(cause); }
  }
  async function afterSave(outcome, target) {
    const active = await tracker.afterSave(outcome, target);
    if (active && outcome?.redirectFamily) notice = c.redirectedCopy;
    return active;
  }
  const poll = () => tracker.poll();
  async function startJoin() {
    await run(async () => {
      const value = invitationInput;
      invitationInput = '';
      await tracker.startJoin(value);
    });
  }
  const confirmJoin = () => run(() => tracker.confirmJoin());
  const dismissJoin = () => run(() => tracker.dismissJoin());
  async function makeFamily() {
    await run(async () => { if (await tracker.makeFamily()) editProfile(null); });
  }
  async function selectFamily(value) {
    screen = ''; editing = null;
    // onSelect already opened Today; a later tab choice must survive the load.
    await run(() => tracker.selectFamily(value));
  }
  const makeRemovedCopy = () => run(() => tracker.makeRemovedCopy());
  async function openPrivateCopy() {
    if (privateCopy) await selectFamily(privateCopy);
  }
  // An independent local Family from the current shared state; access is unchanged.
  async function makeSharedCopy() {
    await run(async () => {
      if (await tracker.restoreBackup(await api.exportFamily(family))) { tab = 'today'; notice = c.privateCopyReady; }
    });
  }
  // First run: the Family is created only once a child is named.
  async function startTracking(name, birthDay) {
    let created = false;
    await run(async () => { created = await tracker.makeFamily(); });
    if (created) await saveProfile([{ type: 'child', name, birthDay, sex: 3 }]);
  }
  async function restoreBackup(readable) {
    if (await tracker.restoreBackup(readable)) { screen = ''; tab = 'today'; }
  }
  function selectChild(value) {
    actionSequence++;
    sheetId = '';
    tracker.selectChild(value);
    screen = ''; editing = null;
  }
  // Save one or more core actions for the current target; true when applied.
  async function saveActions(actions) {
    let saved = false;
    await run(async () => {
      const target = tracker.target();
      for (const action of actions) {
        if (!await afterSave(await api.act(target.family, action), target)) return;
      }
      saved = true;
    });
    return saved;
  }
  const saveEntry = (action) => saveActions([action]);
  // Forms clear their own drafts first, then return to Today.
  function finishEntry() { screen = ''; tab = 'today'; }
  async function saveProfile(actions) {
    const before = new Set(data.children.map((row) => row.id));
    if (!await saveActions(actions)) return;
    const added = $tracker.data.children.find((row) => !before.has(row.id));
    if (added) tracker.selectChild(added.id);
    screen = '';
    profileChild = null;
  }
  // A saved running sleep is shared state; the busy flag blocks a duplicate start or stop.
  async function toggleSleep(running, place = null) {
    if (sleepBusy) return;
    sleepBusy = true;
    try {
      const now = Date.now();
      const saved = await saveActions([running ?
        { type: 'stopSleep', child, target: running.id, endMs: now, endOffset: api.offsetAt(now) } :
        { type: 'sleep', child, startMs: now, offset: api.offsetAt(now), place }]);
      if (saved && screen === 'capture') { screen = ''; tab = 'today'; }
    } finally { sleepBusy = false; }
  }
  const logDiaper = (kind) => saveActions([{ type: 'diaper', child, kind, startMs: Date.now(), offset: api.offsetAt(Date.now()) }]);
  function openEntry(row) { if (!removedInfo) { sheetId = row.id; error = ''; } }
  function begin(kind) {
    error = '';
    if (!kind) { screen = 'chooser'; return; }
    if (kind === 'feed.breast') { screen = 'breast'; return; }
    captureKind = kind;
    screen = 'capture';
  }
  function editProfile(row) { profileChild = row; screen = 'child'; error = ''; }
  function editEntry(row, action) { sheetId = ''; editing = { row, action }; screen = 'edit'; error = ''; }
  async function removeEntry(row) {
    if (!window.confirm(c.deleteConfirm)) return false;
    const target = tracker.target();
    if (await saveActions([{ type: 'delete', child: row.childId, target: row.id }])) {
      undo = { ...target, child: row.childId, id: row.id };
      setTimeout(() => { if (undo?.id === row.id) undo = null; }, 10_000);
      return true;
    }
    return false;
  }
  async function undoDelete() {
    const saved = undo;
    undo = null;
    if (saved && tracker.current(saved)) await saveActions([{ type: 'restore', child: saved.child, target: saved.id }]);
  }
  function formatTime(value) { return new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(value); }
  function formatDay(value) { return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(value); }
</script>

{#snippet joinPanel()}
  <form class="join-form" onsubmit={(event) => { event.preventDefault(); startJoin(); }}>
    <label>{c.invitationLink}<input type="text" inputmode="url" required bind:value={invitationInput} /></label>
    <button class="secondary" type="submit">{c.join}</button>
  </form>
  {#if pendingFragment}<p class="join-stage" role="status">{joinStage || c.joining}</p>
    {#if joinStage === c.confirmJoin}<p class="muted">{c.joinHistoryWarning}</p><button class="primary" onclick={confirmJoin}>{c.joinThisFamily}</button><button class="text-action" onclick={dismissJoin}>{c.dismissInvitation}</button>
    {:else if terminalJoinStages.includes(joinStage)}<button class="text-action" onclick={dismissJoin}>{c.dismissInvitation}</button>
    {:else}<button class="text-action" onclick={poll}>{c.pendingJoinResume}</button>{/if}{/if}
{/snippet}

<svelte:head>
  <title>{c.app} · {c.preview}</title>
</svelte:head>

<div class="app-shell">
  <aside class="side-nav" aria-label="Primary navigation">
    <div class="wordmark"><span class="mark"><Icon name="mark" size={22} /></span>{c.app}</div>
    {#if family}
      <button class:active={tab === 'today' && !screen} onclick={() => { screen = ''; tab = 'today'; }}>{c.today}</button>
      <button class:active={tab === 'history' && !screen} onclick={() => { screen = ''; tab = 'history'; }}>{c.history}</button>
      <button class:active={tab === 'family' && !screen} onclick={() => { screen = ''; tab = 'family'; }}>{c.family}</button>
    {/if}
    <small>{sharedSelected ? c.sharedNotice : c.localNotice}</small>
  </aside>

  <div class="main-column">
    {#if family && !screen}<header class="topbar">
      <AppHeader child={selectedChild} children={data.children} familyLabel={familyRows.length > 1 ? familyLabel : ''}
        shared={sharedSelected} {syncStage} {selectChild} />
    </header>{/if}

    <main>
      {#if error}<div class="error" role="alert">{error}</div>{/if}
      {#if notice}<div class="notice" role="status">{notice}</div>{/if}
      {#if undo}<div class="notice undo" role="status"><span>{c.entryDeleted}</span><button class="text-action" onclick={undoDelete}>{c.undo}</button></div>{/if}
      {#if loading}
        <p class="muted">{c.loading}</p>
      {:else if !family}
        {#if screen === 'onboard'}
          <FirstChild start={startTracking} back={() => screen = ''} />
        {:else if screen === 'join' || pendingFragment}
          <section class="form-view"><button class="back" onclick={() => screen = ''}>← {c.back}</button>
            <h1>{c.joinFamily}</h1><p class="muted">{c.invitationHint}</p>{@render joinPanel()}</section>
        {:else if screen === 'restore'}
          <section class="form-view"><button class="back" onclick={() => screen = ''}>← {c.back}</button>
            <h1>{c.restoreBackup}</h1><p class="muted">{c.restoreDescription}</p><BackupPanel restore={restoreBackup} /></section>
        {:else if screen === 'privacy'}
          <section class="form-view"><button class="back" onclick={() => screen = ''}>← {c.back}</button>
            <h1>{c.privacyTitle}</h1><p>{c.privacyBody}</p></section>
        {:else}
          <Welcome addChild={() => screen = 'onboard'} join={() => screen = 'join'}
            restore={() => screen = 'restore'} privacy={() => screen = 'privacy'} />
        {/if}
      {:else if screen === 'child'}
        <section class="form-view">
          <button class="back" onclick={() => { screen = ''; profileChild = null; }}>← {c.cancel}</button>
          <div class="eyebrow">{c.family}</div>
          <h1>{profileChild ? c.editChild : c.addChild}</h1>
          <ChildProfile child={profileChild} save={saveProfile} cancel={() => { screen = ''; profileChild = null; }} />
        </section>
      {:else if screen === 'chooser'}
        <section>
          <button class="back" onclick={() => screen = ''}>← {c.back}</button>
          <div class="eyebrow">{c.forChild(selectedChild?.name)}</div>
          <h1>{c.addActivityTitle}</h1>
          {#each chooser as group}
            <h2>{group.title}</h2>
            <div class="quick-grid">
              {#each group.kinds as kind}
                <button class={category(kind)} onclick={() => begin(kind)}><Icon name={iconFor(kind)} size={28} /><strong>{kindLabels[kind]}</strong></button>
              {/each}
            </div>
          {/each}
        </section>
      {:else if screen === 'breast'}
        <section class="form-view breast-view">
          <button class="back" onclick={() => screen = ''}>← {c.back}</button>
          <div class="eyebrow">{c.forChild(selectedChild?.name)}</div>
          <h1>{c.breastFeed}</h1>
          {#key `${family}:${child}`}<BreastFeed {family} {child} {entries} {nowMs} save={saveEntry} done={finishEntry} />{/key}
        </section>
      {:else if screen === 'edit' && editing}
        <section class="form-view">
          <button class="back" onclick={() => { screen = ''; editing = null; }}>← {c.cancel}</button>
          <div class="eyebrow">{selectedChild?.name} · {formatDay(editing.row.startMs)}</div>
          <h1>{entrySummary(editing.row)}</h1>
          {#key editing}<EntryEditor row={editing.row} action={editing.action} child={editing.row.childId}
            save={saveEntry} done={() => { screen = ''; editing = null; }} />{/key}
        </section>
      {:else if screen === 'capture'}
        <section class="form-view">
          <button class="back" onclick={() => screen = ''}>← {c.cancel}</button>
          <div class="eyebrow">{c.forChild(selectedChild?.name)}</div>
          <h1>{kindLabels[captureKind]}</h1>
          {#key `${family}:${child}:${captureKind}`}<Capture kind={captureKind} {family} {child} {entries} {nowMs}
            save={saveEntry} done={finishEntry} startSleep={(place) => toggleSleep(null, place)} />{/key}
        </section>
      {:else if tab === 'today'}
        <section>
          {#if removedInfo}<div class="panel" role="status"><strong>{c.removedTitle}</strong><p>{c.removedDetail}</p>
            {#if privateCopy}<button class="primary" onclick={openPrivateCopy}>{c.openPrivateCopy}</button>
            {:else}<button class="secondary" onclick={makeRemovedCopy}>{c.makePrivateCopy}</button>{/if}
          </div>{/if}
          {#if selectedChild}
            {#if !removedInfo}
              <Today {entries} summary={todaySummary} {breastDraft} {pumpStartMs} {nowMs} busy={sleepBusy} open={begin}
                startSleep={() => toggleSleep(null)} {openEntry} {logDiaper} viewHistory={() => tab = 'history'} />
            {:else}
              {#each entries.slice(0, 3) as row (row.id)}<p>{entrySummary(row)}</p>{/each}
            {/if}
          {:else}
            <div class="panel empty"><p>{c.addChildPrompt}</p>{#if !removedInfo}<button class="primary" onclick={() => editProfile(null)}>{c.addChild}</button>{/if}</div>
          {/if}
        </section>
      {:else if tab === 'history'}
        <section><h1 class="screen-title">{c.history}</h1>
          {#key `${family}:${child}`}<History {entries} {nowMs} editable={!removedInfo} {loadSummary} open={openEntry} />{/key}
        </section>
      {:else}
        <section><h1 class="screen-title">{c.family}</h1>
          <div class="panel"><h2>{c.children}</h2>
            {#each data.children as row}<div class="child-row"><span class="avatar">{row.name.slice(0, 1).toUpperCase()}</span><div><strong>{row.name}</strong><small>{ageLabel(row.birthDay)}</small></div>
              {#if row.id !== child}<button class="text-action" onclick={() => selectChild(row.id)}>{c.select}</button>{/if}
              {#if !removedInfo}<button class="entry-edit" aria-label={c.editChildNamed(row.name)} onclick={() => editProfile(row)}>{c.edit}</button>{/if}</div>{/each}
            {#if !removedInfo}<button class="secondary" onclick={() => editProfile(null)}>＋ {data.children.length ? c.addAnotherChild : c.addChild}</button>{/if}
          </div>
          <div class="panel"><h2>{sharedSelected ? c.shared : c.localOnly}</h2><p class="muted">{sharedSelected ? c.sharedDescription : c.localDescription}</p>
            {#if sharedSelected}<p role="status">{syncStage || (removedInfo ? c.removedArchive : c.syncReady)}</p>{#if !removedInfo}<button class="secondary" onclick={poll}>{c.syncNow}</button>
              {#key family}<SharingPanel {family} {devices} {deviceId} makeCopy={makeSharedCopy} />{/key}{/if}{/if}
            <label class="family-switch">{c.switchFamily}<select value={family} onchange={(event) => selectFamily(event.currentTarget.value)}>
              {#each familyRows as row, index}<option value={row.family}>{c.familyNumber(index + 1)}</option>{/each}
            </select></label>
            <button class="secondary" onclick={makeFamily}>＋ {c.createFamily}</button>
          </div>
          <div class="panel"><h2>{c.dataAndBackups}</h2>
            {#key family}<BackupPanel {family} shared={sharedSelected} {backupCursor} {backupGap} restore={restoreBackup} />{/key}
          </div>
          <div class="panel join-panel"><h2>{c.joinFamily}</h2><p class="muted">{c.invitationHint}</p>{@render joinPanel()}</div>
        </section>
      {/if}
    </main>
    {#if sheetRow && !screen}
      <EntrySheet row={sheetRow} {nowMs} busy={sleepBusy} save={saveEntry} edit={editEntry} close={() => sheetId = ''}
        stopSleep={async (entry) => { await toggleSleep(entry); sheetId = ''; }}
        remove={async (entry) => { if (await removeEntry(entry)) sheetId = ''; }} />
    {/if}
    {#if family && !screen}<nav class="bottom-nav" aria-label="Primary navigation">
      <button class:active={tab === 'today'} onclick={() => tab = 'today'}>{c.today}</button>
      <button class:active={tab === 'history'} onclick={() => tab = 'history'}>{c.history}</button>
      <button class:active={tab === 'family'} onclick={() => tab = 'family'}>{c.family}</button>
    </nav>{/if}
  </div>
</div>
