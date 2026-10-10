<script>
  import { onMount } from 'svelte';
  import { copy as c, ageLabel, sleepDuration } from './strings.js';
  import * as api from './core.js';
  import { createTrackerController } from './tracker-controller.js';
  import { readDraft, persistDraft, tapSide, completedSegments, sideTotals,
    durationLabel, editableSegments, rebuiltSegments } from './breast-timer.js';

  let error = '';
  let notice = '';
  let tab = 'today';
  let screen = '';
  let childName = '';
  let birthDate = '';
  let sex = '';
  let activityType = '';
  let diaperKind = '1';
  let bottleMl = '';
  let bottleContent = '1';
  let noteText = '';
  let breastDraft = null;
  let nowMs = Date.now();
  let editTarget = null;
  let editRows = [];
  let invitationInput = '';
  let actionSequence = 0;
  let sleepBusy = false;
  const terminalJoinStages = [c.inviteClaimed, c.inviteCanceled, c.inviteExpired, c.inviteInvalidated];

  $: sharedSelected = $tracker.familyRows.find((row) => row.family === $tracker.family)?.source === 'shared';
  $: selectedChild = data.children.find((row) => row.id === child);
  $: familyLabel = c.familyNumber(familyRows.findIndex((row) => row.family === family) + 1);
  $: entries = data.activities.filter((row) => row.childId === child);
  $: todayEntries = entries.filter((row) => new Date(row.startMs).toDateString() === new Date().toDateString());
  $: runningSleep = entries.find((row) => row.kind === 'sleep' && row.endMs == null);

  const tracker = createTrackerController({ api, copy: c, preferences: localStorage,
    onSelect: () => { screen = ''; editTarget = null; tab = 'today'; },
    onRemoval: () => { screen = ''; editTarget = null; },
    onRememberInvitation: () => history.replaceState(null, '', location.pathname + location.search) });
  $: ({ loading, familyRows, family, child, data, pendingFragment, joinStage,
    syncStage, removedInfo, privateCopy, backupGap, backupCursor } = $tracker);
  $: breastDraft = $tracker.family && $tracker.child ? readDraft($tracker.family, $tracker.child) : null;

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
    await run(async () => { if (await tracker.makeFamily()) screen = 'child'; });
  }
  async function selectFamily(value) {
    screen = ''; editTarget = null;
    await run(async () => { await tracker.selectFamily(value); tab = 'today'; });
  }
  const makeRemovedCopy = () => run(() => tracker.makeRemovedCopy());
  async function openPrivateCopy() {
    if (privateCopy) await selectFamily(privateCopy);
  }
  async function downloadBackup() {
    await run(async () => {
      const targetFamily = family;
      const readable = await api.exportFamily(targetFamily);
      const url = URL.createObjectURL(new Blob([readable], { type: 'application/x-ndjson' }));
      const link = document.createElement('a');
      link.href = url;
      link.download = `babytrack-${targetFamily.slice(0, 8)}.jsonl`;
      document.body.append(link);
      link.click();
      link.remove();
      setTimeout(() => URL.revokeObjectURL(url), 60000);
    });
  }
  async function restoreBackup(file) {
    if (!file) return;
    await run(async () => {
      if (await tracker.restoreBackup(file)) { screen = ''; tab = 'today'; }
    });
  }
  function selectChild(value) {
    actionSequence++;
    tracker.selectChild(value);
    screen = ''; editTarget = null;
  }
  async function saveChild() {
    await run(async () => {
      const target = tracker.target();
      const outcome = await api.addChild(target.family, childName, birthDate, sex);
      if (!await afterSave(outcome, target)) return;
      tracker.selectChild($tracker.data.children.at(-1)?.id || $tracker.child);
      childName = ''; birthDate = ''; sex = '';
      screen = '';
    });
  }
  async function saveActivity() {
    await run(async () => {
      const target = tracker.target();
      const outcome = await api.logActivity(target.family, target.child, activityType, {
        kind: diaperKind, ml: bottleMl, content: bottleContent, note: noteText,
      });
      if (!await afterSave(outcome, target)) return;
      bottleMl = ''; noteText = '';
      screen = '';
      tab = 'today';
    });
  }
  // A saved running sleep is shared state; the busy flag blocks a duplicate start or stop.
  async function toggleSleep(running) {
    if (sleepBusy) return;
    sleepBusy = true;
    try {
      await run(async () => {
        const target = tracker.target();
        const outcome = running ? await api.stopSleep(target.family, target.child, running.id)
          : await api.startSleep(target.family, target.child);
        await afterSave(outcome, target);
      });
    } finally { sleepBusy = false; }
  }
  function begin(type) { activityType = type; screen = 'capture'; error = ''; }
  function openBreast() { breastDraft = readDraft(family, child); screen = 'breast'; error = ''; }
  function timerError(cause) { return ({
    'too-short': c.timerTooShort,
    'duration-limit': c.timerTooLong,
    'segment-limit': c.timerTooManySegments,
    'empty-timer': c.timerEmpty,
    'invalid-duration': c.invalidDuration,
    'future-end': c.futureEnd,
  })[cause?.message] || message(cause); }
  function tapBreast(side) {
    error = '';
    try {
      const next = tapSide(breastDraft, side);
      persistDraft(family, child, next);
      breastDraft = next;
    } catch (cause) { error = timerError(cause); }
  }
  async function saveBreast() {
    error = '';
    notice = '';
    const target = tracker.target();
    const targetFamily = target.family;
    const targetChild = target.child;
    try {
      const segments = completedSegments(breastDraft);
      const outcome = await api.logBreastFeed(targetFamily, targetChild, segments);
      persistDraft(targetFamily, targetChild, null);
      if (!await afterSave(outcome, target)) return;
      screen = '';
      tab = 'today';
    } catch (cause) { error = timerError(cause); }
  }
  function discardBreast() {
    if (!window.confirm(c.discardBreastConfirm)) return;
    persistDraft(family, child, null);
    breastDraft = null;
    screen = '';
  }
  function beginBreastEdit(row) {
    editTarget = { ...tracker.target(), id: row.id, segments: row.breastSegments };
    editRows = editableSegments(row.breastSegments);
    screen = 'breast-edit';
    error = '';
  }
  function updateEditRow(index, field, value) {
    editRows = editRows.map((row, position) => position === index ? { ...row, [field]: value } : row);
  }
  async function saveBreastEdit() {
    error = '';
    notice = '';
    try {
      const target = editTarget;
      const segments = rebuiltSegments(target.segments, editRows);
      const outcome = await api.editBreastFeed(target.family, target.child, target.id, segments);
      if (!await afterSave(outcome, target)) return;
      editTarget = null;
      screen = '';
      tab = 'history';
    } catch (cause) { error = timerError(cause); }
  }
  function formatTime(value) { return new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(value); }
  function formatDay(value) { return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(value); }
  function entryLabel(row) {
    if (row.kind === 'diaper') return `${c.diaper} · ${[c.wet, c.dirty, c.both, c.dry][row.diaperKind - 1] || c.diaper}`;
    if (row.kind === 'feed.bottle') return `${c.bottle} · ${row.bottleMl ?? '—'} mL`;
    if (row.kind === 'feed.breast' && row.breastSegments) {
      const totals = sideTotals({ segments: row.breastSegments });
      return c.breastEntry(durationLabel(totals[1]), durationLabel(totals[2]));
    }
    if (row.kind === 'note') return row.note || c.note;
    if (row.kind === 'sleep') return row.endMs == null ? c.sleepRunning : c.sleepEntry(sleepDuration(row.endMs - row.startMs));
    return row.kind;
  }
</script>

<svelte:head>
  <title>{c.app} · {c.preview}</title>
</svelte:head>

<div class="app-shell">
  <aside class="side-nav" aria-label="Primary navigation">
    <div class="wordmark">◌ <span>{c.app}</span></div>
    {#if family}
      <button class:active={tab === 'today' && !screen} onclick={() => { screen = ''; tab = 'today'; }}>{c.today}</button>
      <button class:active={tab === 'history' && !screen} onclick={() => { screen = ''; tab = 'history'; }}>{c.history}</button>
      <button class:active={tab === 'family' && !screen} onclick={() => { screen = ''; tab = 'family'; }}>{c.family}</button>
    {/if}
    <small>{sharedSelected ? c.sharedNotice : c.localNotice}</small>
  </aside>

  <div class="main-column">
    <header class="topbar">
      <div class="brand-mobile">◌ {c.app}</div>
      {#if family && !screen}
        <div class="target">
          <span>{familyLabel}</span>
          <select aria-label={c.chooseChild} value={child} onchange={(event) => selectChild(event.currentTarget.value)}>
            {#each data.children as row}<option value={row.id}>{row.name}</option>{/each}
          </select>
        </div>
      {/if}
    </header>

    <main>
      {#if error}<div class="error" role="alert">{error}</div>{/if}
      {#if notice}<div class="notice" role="status">{notice}</div>{/if}
      {#if loading}
        <p class="muted">{c.loading}</p>
      {:else if !family}
        <section class="welcome panel">
          <div class="eyebrow">{c.preview}</div>
          <h1>{c.welcomeTitle}</h1>
          <p>{c.noAccount} {c.welcomeDetail}</p>
          <button class="primary" onclick={makeFamily}>{c.createFamily}</button>
        </section>
        <section class="panel join-panel"><h2>{c.joinFamily}</h2><p class="muted">{c.invitationHint}</p>
          <form onsubmit={(event) => { event.preventDefault(); startJoin(); }}>
            <label>{c.invitationLink}<input type="text" inputmode="url" required bind:value={invitationInput} /></label>
            <button class="secondary" type="submit">{c.join}</button>
          </form>
          {#if pendingFragment}<p role="status">{joinStage || c.joining}</p>
            {#if joinStage === c.confirmJoin}<p class="muted">{c.joinHistoryWarning}</p><button class="primary" onclick={confirmJoin}>{c.joinThisFamily}</button><button class="text-action" onclick={dismissJoin}>{c.dismissInvitation}</button>
            {:else if terminalJoinStages.includes(joinStage)}<button class="text-action" onclick={dismissJoin}>{c.dismissInvitation}</button>
            {:else}<button class="text-action" onclick={poll}>{c.pendingJoinResume}</button>{/if}{/if}
        </section>
        <section class="panel"><h2>{c.restoreBackup}</h2><p class="muted">{c.restoreDescription}</p>
          <label>{c.restoreFile}<input type="file" accept=".jsonl,.json" onchange={(event) => {
            const file = event.currentTarget.files?.[0];
            event.currentTarget.value = '';
            restoreBackup(file);
          }} /></label>
        </section>
      {:else if screen === 'child'}
        <section class="form-view">
          <button class="back" onclick={() => screen = ''}>← {c.cancel}</button>
          <div class="eyebrow">{c.family}</div>
          <h1>{c.addChild}</h1>
          <form onsubmit={(event) => { event.preventDefault(); saveChild(); }}>
            <label>{c.childName}<input required maxlength="160" bind:value={childName} autocomplete="off" /></label>
            <label>{c.birthDate}<input type="date" max={new Date().toISOString().slice(0, 10)} bind:value={birthDate} /></label>
            <label>{c.growthSex}<select bind:value={sex}>
              <option value="">{c.unspecified}</option><option value="1">{c.female}</option>
              <option value="2">{c.male}</option><option value="3">{c.other}</option>
            </select></label>
            <button class="primary save" type="submit">{c.save}</button>
          </form>
        </section>
      {:else if screen === 'breast'}
        <section class="form-view breast-view">
          <button class="back" onclick={() => screen = ''}>← {c.back}</button>
          <div class="eyebrow">{selectedChild?.name} · {c.feeds}</div>
          <h1>{c.breastFeed}</h1>
          <p class="muted">{breastDraft?.active ? c.feedingNow : breastDraft?.segments.length ? c.feedingPaused : c.tapSideToStart}</p>
          <div class="breast-totals panel">
            <div><span>{c.leftBreast}</span><strong>{durationLabel(sideTotals(breastDraft, nowMs)[1])}</strong></div>
            <div><span>{c.rightBreast}</span><strong>{durationLabel(sideTotals(breastDraft, nowMs)[2])}</strong></div>
          </div>
          <div class="breast-controls">
            <button class:running={breastDraft?.active?.side === 1} aria-pressed={breastDraft?.active?.side === 1} onclick={() => tapBreast(1)}>
              <span class="breast-control-symbol" aria-hidden="true">{breastDraft?.active?.side === 1 ? 'Ⅱ' : '▶'}</span>
              <strong>{c.leftBreast}</strong>
              <small>{breastDraft?.active?.side === 1 ? c.tapToPause : c.tapToStart}</small>
            </button>
            <button class:running={breastDraft?.active?.side === 2} aria-pressed={breastDraft?.active?.side === 2} onclick={() => tapBreast(2)}>
              <span class="breast-control-symbol" aria-hidden="true">{breastDraft?.active?.side === 2 ? 'Ⅱ' : '▶'}</span>
              <strong>{c.rightBreast}</strong>
              <small>{breastDraft?.active?.side === 2 ? c.tapToPause : c.tapToStart}</small>
            </button>
          </div>
          <p class="muted">{c.breastTimerHint}</p>
          {#if breastDraft?.segments.length}
            <div class="panel segment-list"><h2>{c.completedSides}</h2>
              {#each breastDraft.segments as segment}
                <div>{segment.side === 1 ? c.leftBreast : c.rightBreast} · {durationLabel(segment.end_utc_ms - segment.start_utc_ms)}</div>
              {/each}
            </div>
          {/if}
          <button class="primary save" disabled={!breastDraft} onclick={saveBreast}>{c.saveBreast}</button>
          {#if breastDraft}<button class="text-action discard" onclick={discardBreast}>{c.discardSession}</button>{/if}
        </section>
      {:else if screen === 'breast-edit'}
        <section class="form-view">
          <button class="back" onclick={() => { screen = ''; editTarget = null; }}>← {c.cancel}</button>
          <div class="eyebrow">{selectedChild?.name} · {formatDay(editTarget.segments[0].start_utc_ms)}</div>
          <h1>{c.editBreast}</h1>
          <p class="muted">{c.editBreastHint}</p>
          <form onsubmit={(event) => { event.preventDefault(); saveBreastEdit(); }}>
            {#each editRows as row, index}
              <div class="panel edit-segment">
                <h2>{c.sideNumber(index + 1)}</h2>
                <label>{c.side}<select value={row.side} onchange={(event) => updateEditRow(index, 'side', event.currentTarget.value)}>
                  <option value="1">{c.leftBreast}</option><option value="2">{c.rightBreast}</option>
                </select></label>
                {#if index > 0}<label>{c.pauseBefore}<input inputmode="numeric" value={row.pause} oninput={(event) => updateEditRow(index, 'pause', event.currentTarget.value)} required /></label>{/if}
                <label>{c.durationMmSs}<input inputmode="numeric" value={row.duration} oninput={(event) => updateEditRow(index, 'duration', event.currentTarget.value)} required /></label>
                {#if editRows.length > 1}<button type="button" class="text-action" onclick={() => editRows = editRows.filter((_, position) => position !== index)}>{c.removeSide}</button>{/if}
              </div>
            {/each}
            {#if editRows.length < 8}<button type="button" class="secondary" onclick={() => editRows = [...editRows, { side: editRows.at(-1)?.side === '1' ? '2' : '1', pause: '0:00', duration: '5:00' }]}>{c.addSide}</button>{/if}
            <button class="primary save" type="submit">{c.saveChanges}</button>
          </form>
        </section>
      {:else if screen === 'capture'}
        <section class="form-view">
          <button class="back" onclick={() => screen = ''}>← {c.cancel}</button>
          <div class="eyebrow">{selectedChild?.name} · {formatTime(Date.now())}</div>
          <h1>{activityType === 'diaper' ? c.diaper : activityType === 'bottle' ? c.bottle : c.note}</h1>
          <form onsubmit={(event) => { event.preventDefault(); saveActivity(); }}>
            {#if activityType === 'diaper'}
              <label>{c.type}<select bind:value={diaperKind}>
                <option value="1">{c.wet}</option><option value="2">{c.dirty}</option>
                <option value="3">{c.both}</option><option value="4">{c.dry}</option>
              </select></label>
            {:else if activityType === 'bottle'}
              <label>{c.amount}<input type="number" inputmode="numeric" min="1" max="1000000" step="1" required bind:value={bottleMl} /></label>
              <label>{c.content}<select bind:value={bottleContent}>
                <option value="1">{c.milk}</option><option value="2">{c.formula}</option>
                <option value="3">{c.mixed}</option><option value="4">{c.otherMilk}</option>
              </select></label>
            {:else}
              <label>{c.noteText}<textarea rows="5" maxlength="4096" required bind:value={noteText}></textarea></label>
            {/if}
            <button class="primary save" type="submit">{c.save}</button>
          </form>
        </section>
      {:else if tab === 'today'}
        <section>
          {#if removedInfo}<div class="panel" role="status"><strong>{c.removedTitle}</strong><p>{c.removedDetail}</p>
            {#if privateCopy}<button class="primary" onclick={openPrivateCopy}>{c.openPrivateCopy}</button>
            {:else}<button class="secondary" onclick={makeRemovedCopy}>{c.makePrivateCopy}</button>{/if}
          </div>{/if}
          <div class="eyebrow">{formatDay(Date.now())}</div>
          <h1>{selectedChild ? selectedChild.name : c.today}</h1>
          <p class="muted">{selectedChild ? ageLabel(selectedChild.birthDay) : c.chooseChild}</p>
          {#if selectedChild}
            <div class="summary panel"><span>{c.todaySummary}</span><strong>{c.feedCount(todayEntries.filter((row) => row.kind.startsWith('feed.')).length)} · {c.diaperCount(todayEntries.filter((row) => row.kind === 'diaper').length)}</strong></div>
            {#if breastDraft && !removedInfo}
              <button class="timer-resume panel" onclick={openBreast}><span>{c.breastFeed} · {breastDraft.active ? c.feedingNow : c.feedingPaused}</span><strong>{durationLabel(sideTotals(breastDraft, nowMs)[1] + sideTotals(breastDraft, nowMs)[2])} →</strong></button>
            {/if}
            {#if runningSleep && !removedInfo}
              <div class="timer-resume panel sleep-running"><span>{c.sleepingSince(formatTime(runningSleep.startMs))}</span><strong>{durationLabel(nowMs - runningSleep.startMs)}</strong>
                <button class="secondary" disabled={sleepBusy} onclick={() => toggleSleep(runningSleep)}>{c.stopSleep}</button></div>
            {/if}
            {#if !removedInfo}<h2>{c.addActivity}</h2>
            <div class="quick-grid">
              <button onclick={openBreast}><span class="icon feed" aria-hidden="true">◓</span><strong>{c.breastFeed}</strong><small>{c.feeds}</small></button>
              <button onclick={() => begin('bottle')}><span class="icon feed" aria-hidden="true">◔</span><strong>{c.bottle}</strong><small>{c.feeds}</small></button>
              <button onclick={() => begin('diaper')}><span class="icon care" aria-hidden="true">◇</span><strong>{c.diaper}</strong><small>{c.diapers}</small></button>
              {#if !runningSleep}<button disabled={sleepBusy} onclick={() => toggleSleep(null)}><span class="icon sleep" aria-hidden="true">☾</span><strong>{c.sleep}</strong><small>{c.startSleep}</small></button>{/if}
              <button onclick={() => begin('note')}><span class="icon note" aria-hidden="true">✎</span><strong>{c.note}</strong><small>{c.addActivity}</small></button>
            </div>{/if}
            <div class="section-heading"><h2>{c.recent}</h2><button class="text-action" onclick={() => tab = 'history'}>{c.allEntries} →</button></div>
            {#if entries.length === 0}<p class="muted">{c.emptyHistory}</p>{/if}
            {#each entries.slice(0, 4) as row}
              <div class="entry"><span class="entry-dot"></span><div><strong>{entryLabel(row)}</strong><small>{formatDay(row.startMs)}</small></div><time>{formatTime(row.startMs)}</time>{#if !removedInfo && row.kind === 'feed.breast' && row.breastSegments?.length}<button class="entry-edit" onclick={() => beginBreastEdit(row)}>{c.edit}</button>{/if}</div>
            {/each}
          {:else}
            <div class="panel empty"><p>{c.addChildPrompt}</p>{#if !removedInfo}<button class="primary" onclick={() => screen = 'child'}>{c.addChild}</button>{/if}</div>
          {/if}
        </section>
      {:else if tab === 'history'}
        <section><div class="eyebrow">{selectedChild?.name || c.family}</div><h1>{c.history}</h1>
          {#if entries.length === 0}<div class="panel empty">{c.emptyHistory}</div>{/if}
          {#each entries as row}
            <div class="entry"><span class="entry-dot"></span><div><strong>{entryLabel(row)}</strong><small>{formatDay(row.startMs)}</small></div><time>{formatTime(row.startMs)}</time>{#if !removedInfo && row.kind === 'feed.breast' && row.breastSegments?.length}<button class="entry-edit" onclick={() => beginBreastEdit(row)}>{c.edit}</button>{/if}</div>
          {/each}
        </section>
      {:else}
        <section><div class="eyebrow">{sharedSelected ? c.shared : c.localOnly}</div><h1>{c.family}</h1>
          <div class="panel"><h2>{c.children}</h2>
            {#each data.children as row}<div class="child-row"><span class="avatar">{row.name.slice(0, 1).toUpperCase()}</span><div><strong>{row.name}</strong><small>{ageLabel(row.birthDay)}</small></div></div>{/each}
            {#if !removedInfo}<button class="secondary" onclick={() => screen = 'child'}>＋ {c.addChild}</button>{/if}
          </div>
          <div class="panel"><h2>{sharedSelected ? c.shared : c.localOnly}</h2><p class="muted">{sharedSelected ? c.sharedDescription : c.localDescription}</p>
            {#if sharedSelected}<p role="status">{syncStage || (removedInfo ? c.removedArchive : c.syncReady)}</p>{#if !removedInfo}<button class="secondary" onclick={poll}>{c.syncNow}</button>{/if}{/if}
            <label class="family-switch">{c.switchFamily}<select value={family} onchange={(event) => selectFamily(event.currentTarget.value)}>
              {#each familyRows as row, index}<option value={row.family}>{c.familyNumber(index + 1)}</option>{/each}
            </select></label>
            <button class="secondary" onclick={makeFamily}>＋ {c.createFamily}</button>
          </div>
          <div class="panel"><h2>{c.backup}</h2><p class="muted">{c.backupDescription}</p>
            {#if sharedSelected}<p class="muted">{c.backupPoint(backupCursor)} {backupGap ? c.backupIncomplete : c.backupComplete}</p>{/if}
            <button class="secondary" onclick={downloadBackup}>{c.exportBackup}</button>
            <label>{c.restoreBackup}<input type="file" accept=".jsonl,.json" onchange={(event) => {
              const file = event.currentTarget.files?.[0];
              event.currentTarget.value = '';
              restoreBackup(file);
            }} /></label>
          </div>
          <div class="panel join-panel"><h2>{c.joinFamily}</h2><p class="muted">{c.invitationHint}</p>
            <form onsubmit={(event) => { event.preventDefault(); startJoin(); }}>
              <label>{c.invitationLink}<input type="text" inputmode="url" required bind:value={invitationInput} /></label>
              <button class="secondary" type="submit">{c.join}</button>
            </form>
            {#if pendingFragment}<p role="status">{joinStage || c.joining}</p>
              {#if joinStage === c.confirmJoin}<p class="muted">{c.joinHistoryWarning}</p><button class="primary" onclick={confirmJoin}>{c.joinThisFamily}</button><button class="text-action" onclick={dismissJoin}>{c.dismissInvitation}</button>
              {:else if terminalJoinStages.includes(joinStage)}<button class="text-action" onclick={dismissJoin}>{c.dismissInvitation}</button>
              {:else}<button class="text-action" onclick={poll}>{c.pendingJoinResume}</button>{/if}{/if}
          </div>
        </section>
      {/if}
    </main>
    {#if family && !screen}<nav class="bottom-nav" aria-label="Primary navigation">
      <button class:active={tab === 'today'} onclick={() => tab = 'today'}>{c.today}</button>
      <button class:active={tab === 'history'} onclick={() => tab = 'history'}>{c.history}</button>
      <button class:active={tab === 'family'} onclick={() => tab = 'family'}>{c.family}</button>
    </nav>{/if}
  </div>
</div>
