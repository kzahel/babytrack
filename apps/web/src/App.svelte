<script>
  import { onMount } from 'svelte';
  import { copy as c, ageLabel } from './strings.js';
  import { families, createFamily, snapshot, addChild, logActivity, logBreastFeed, editBreastFeed } from './core.js';
  import { readDraft, persistDraft, tapSide, completedSegments, sideTotals,
    durationLabel, editableSegments, rebuiltSegments } from './breast-timer.js';

  let loading = true;
  let error = '';
  let familyRows = [];
  let family = '';
  let child = '';
  let data = { children: [], activities: [] };
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

  $: selectedChild = data.children.find((row) => row.id === child);
  $: familyLabel = c.familyNumber(familyRows.findIndex((row) => row.family === family) + 1);
  $: entries = data.activities.filter((row) => row.childId === child);
  $: todayEntries = entries.filter((row) => new Date(row.startMs).toDateString() === new Date().toDateString());

  onMount(() => {
    const clock = setInterval(() => nowMs = Date.now(), 1000);
    (async () => {
      try {
        familyRows = await families();
        family = familyRows.find((row) => row.family === localStorage.getItem('babytrack-family'))?.family || familyRows[0]?.family || '';
        if (family) await refresh();
      } catch (cause) { error = message(cause); }
      loading = false;
    })();
    return () => clearInterval(clock);
  });

  function message(cause) { return cause?.message || String(cause); }
  async function run(action) {
    error = '';
    try { await action(); }
    catch (cause) { error = message(cause); }
  }
  async function refresh() {
    data = await snapshot(family);
    child = data.children.find((row) => row.id === child)?.id || data.children[0]?.id || '';
    breastDraft = readDraft(family, child);
  }
  async function makeFamily() {
    await run(async () => {
      family = await createFamily();
      localStorage.setItem('babytrack-family', family);
      familyRows = await families();
      await refresh();
      screen = 'child';
    });
  }
  async function selectFamily(value) {
    await run(async () => {
      family = value;
      localStorage.setItem('babytrack-family', family);
      await refresh();
      tab = 'today';
    });
  }
  function selectChild(value) {
    child = value;
    breastDraft = readDraft(family, child);
    screen = '';
  }
  async function saveChild() {
    await run(async () => {
      await addChild(family, childName, birthDate, sex);
      await refresh();
      child = data.children.at(-1)?.id || child;
      childName = ''; birthDate = ''; sex = '';
      screen = '';
    });
  }
  async function saveActivity() {
    await run(async () => {
      await logActivity(family, child, activityType, {
        kind: diaperKind, ml: bottleMl, content: bottleContent, note: noteText,
      });
      await refresh();
      bottleMl = ''; noteText = '';
      screen = '';
      tab = 'today';
    });
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
    const targetFamily = family;
    const targetChild = child;
    try {
      const segments = completedSegments(breastDraft);
      await logBreastFeed(targetFamily, targetChild, segments);
      persistDraft(targetFamily, targetChild, null);
      await refresh();
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
    editTarget = { family, child, id: row.id, segments: row.breastSegments };
    editRows = editableSegments(row.breastSegments);
    screen = 'breast-edit';
    error = '';
  }
  function updateEditRow(index, field, value) {
    editRows = editRows.map((row, position) => position === index ? { ...row, [field]: value } : row);
  }
  async function saveBreastEdit() {
    error = '';
    try {
      const segments = rebuiltSegments(editTarget.segments, editRows);
      await editBreastFeed(editTarget.family, editTarget.child, editTarget.id, segments);
      await refresh();
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
    <small>{c.localNotice}</small>
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
      {#if loading}
        <p class="muted">{c.loading}</p>
      {:else if !family}
        <section class="welcome panel">
          <div class="eyebrow">{c.preview}</div>
          <h1>{c.welcomeTitle}</h1>
          <p>{c.noAccount} {c.welcomeDetail}</p>
          <button class="primary" onclick={makeFamily}>{c.createFamily}</button>
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
              <span class="breast-control-symbol">{breastDraft?.active?.side === 1 ? 'Ⅱ' : '▶'}</span>
              <strong>{c.leftBreast}</strong>
              <small>{breastDraft?.active?.side === 1 ? c.tapToPause : c.tapToStart}</small>
            </button>
            <button class:running={breastDraft?.active?.side === 2} aria-pressed={breastDraft?.active?.side === 2} onclick={() => tapBreast(2)}>
              <span class="breast-control-symbol">{breastDraft?.active?.side === 2 ? 'Ⅱ' : '▶'}</span>
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
              <label>{c.amount}<input type="number" min="1" max="1000000" step="1" required bind:value={bottleMl} /></label>
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
          <div class="eyebrow">{formatDay(Date.now())}</div>
          <h1>{selectedChild ? selectedChild.name : c.today}</h1>
          <p class="muted">{selectedChild ? ageLabel(selectedChild.birthDay) : c.chooseChild}</p>
          {#if selectedChild}
            <div class="summary panel"><span>{c.todaySummary}</span><strong>{todayEntries.filter((row) => row.kind.startsWith('feed.')).length} {c.feeds} · {todayEntries.filter((row) => row.kind === 'diaper').length} {c.diapers}</strong></div>
            {#if breastDraft}
              <button class="timer-resume panel" onclick={openBreast}><span>{c.breastFeed} · {breastDraft.active ? c.feedingNow : c.feedingPaused}</span><strong>{durationLabel(sideTotals(breastDraft, nowMs)[1] + sideTotals(breastDraft, nowMs)[2])} →</strong></button>
            {/if}
            <h2>{c.addActivity}</h2>
            <div class="quick-grid">
              <button onclick={openBreast}><span class="icon feed">◓</span><strong>{c.breastFeed}</strong><small>{c.feeds}</small></button>
              <button onclick={() => begin('bottle')}><span class="icon feed">◔</span><strong>{c.bottle}</strong><small>{c.feeds}</small></button>
              <button onclick={() => begin('diaper')}><span class="icon care">◇</span><strong>{c.diaper}</strong><small>{c.diapers}</small></button>
              <button onclick={() => begin('note')}><span class="icon note">✎</span><strong>{c.note}</strong><small>{c.addActivity}</small></button>
            </div>
            <div class="section-heading"><h2>{c.recent}</h2><button class="text-action" onclick={() => tab = 'history'}>{c.allEntries} →</button></div>
            {#if entries.length === 0}<p class="muted">{c.emptyHistory}</p>{/if}
            {#each entries.slice(0, 4) as row}
              <div class="entry"><span class="entry-dot"></span><div><strong>{entryLabel(row)}</strong><small>{formatDay(row.startMs)}</small></div><time>{formatTime(row.startMs)}</time>{#if row.kind === 'feed.breast' && row.breastSegments?.length}<button class="entry-edit" onclick={() => beginBreastEdit(row)}>{c.edit}</button>{/if}</div>
            {/each}
          {:else}
            <div class="panel empty"><p>{c.addChildPrompt}</p><button class="primary" onclick={() => screen = 'child'}>{c.addChild}</button></div>
          {/if}
        </section>
      {:else if tab === 'history'}
        <section><div class="eyebrow">{selectedChild?.name || c.family}</div><h1>{c.history}</h1>
          {#if entries.length === 0}<div class="panel empty">{c.emptyHistory}</div>{/if}
          {#each entries as row}
            <div class="entry"><span class="entry-dot"></span><div><strong>{entryLabel(row)}</strong><small>{formatDay(row.startMs)}</small></div><time>{formatTime(row.startMs)}</time>{#if row.kind === 'feed.breast' && row.breastSegments?.length}<button class="entry-edit" onclick={() => beginBreastEdit(row)}>{c.edit}</button>{/if}</div>
          {/each}
        </section>
      {:else}
        <section><div class="eyebrow">{c.localOnly}</div><h1>{c.family}</h1>
          <div class="panel"><h2>{c.children}</h2>
            {#each data.children as row}<div class="child-row"><span class="avatar">{row.name.slice(0, 1).toUpperCase()}</span><div><strong>{row.name}</strong><small>{ageLabel(row.birthDay)}</small></div></div>{/each}
            <button class="secondary" onclick={() => screen = 'child'}>＋ {c.addChild}</button>
          </div>
          <div class="panel"><h2>{c.localOnly}</h2><p class="muted">{c.localDescription}</p>
            <label class="family-switch">{c.switchFamily}<select value={family} onchange={(event) => selectFamily(event.currentTarget.value)}>
              {#each familyRows as row, index}<option value={row.family}>{c.familyNumber(index + 1)}</option>{/each}
            </select></label>
            <button class="secondary" onclick={makeFamily}>＋ {c.createFamily}</button>
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
