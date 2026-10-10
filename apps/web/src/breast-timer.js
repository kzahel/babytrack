const prefix = 'babytrack-breast-draft-v1:';
const floorSecond = (ms) => Math.floor(ms / 1000) * 1000;
const offsetAt = (ms) => -new Date(ms).getTimezoneOffset();
const key = (family, child) => `${prefix}${family}:${child}`;

export function readDraft(family, child) {
  if (!family || !child) return null;
  try {
    const draft = JSON.parse(localStorage.getItem(key(family, child)));
    if (!draft || !Array.isArray(draft.segments) || draft.segments.length > 8 ||
        (draft.active && ![1, 2].includes(draft.active.side))) return null;
    return draft;
  } catch { return null; }
}

export function persistDraft(family, child, draft) {
  if (draft) localStorage.setItem(key(family, child), JSON.stringify(draft));
  else localStorage.removeItem(key(family, child));
}

function closeActive(draft, at) {
  const end = floorSecond(at);
  const active = draft.active;
  if (end <= active.start_utc_ms) throw new Error('too-short');
  const segments = [...draft.segments, {
    side: active.side,
    start_utc_ms: active.start_utc_ms,
    end_utc_ms: end,
    start_offset_minutes: active.start_offset_minutes,
    end_offset_minutes: offsetAt(end),
  }];
  if (segments.reduce((sum, row) => sum + row.end_utc_ms - row.start_utc_ms, 0) > 240 * 60_000) {
    throw new Error('duration-limit');
  }
  return { segments, active: null };
}

export function tapSide(draft, side, at = Date.now()) {
  if (![1, 2].includes(side)) throw new Error('invalid-side');
  let next = draft || { segments: [], active: null };
  if (next.active) {
    next = closeActive(next, at);
    if (draft.active.side === side) return next;
  }
  if (next.segments.length >= 8) throw new Error('segment-limit');
  const start = floorSecond(at);
  return { ...next, active: { side, start_utc_ms: start, start_offset_minutes: offsetAt(start) } };
}

export function completedSegments(draft, at = Date.now()) {
  if (!draft) throw new Error('empty-timer');
  const done = draft.active ? closeActive(draft, at) : draft;
  if (!done.segments.length) throw new Error('empty-timer');
  return done.segments;
}

export function sideTotals(draft, at = Date.now()) {
  const totals = { 1: 0, 2: 0 };
  for (const row of draft?.segments || []) totals[row.side] += row.end_utc_ms - row.start_utc_ms;
  if (draft?.active) {
    totals[draft.active.side] += Math.max(0, floorSecond(at) - draft.active.start_utc_ms);
  }
  return totals;
}

export function durationLabel(ms) {
  const seconds = Math.floor(ms / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

export function parseDuration(value, allowZero = false) {
  const match = /^(\d{1,6}):([0-5]\d)$/.exec(value.trim());
  if (!match) throw new Error('invalid-duration');
  const seconds = Number(match[1]) * 60 + Number(match[2]);
  if ((!allowZero && (seconds < 1 || seconds > 14_400)) ||
      (allowZero && seconds > 999_999 * 60 + 59)) throw new Error('invalid-duration');
  return seconds * 1000;
}

export function editableSegments(segments) {
  return segments.map((row, index) => ({
    side: String(row.side),
    duration: durationLabel(row.end_utc_ms - row.start_utc_ms),
    pause: durationLabel(index ? row.start_utc_ms - segments[index - 1].end_utc_ms : 0),
  }));
}

// Rebuild an edited feed. A chosen start moves the whole feed; otherwise
// changed durations keep the original finish (Android's default) or start.
export function rebuiltSegments(original, rows, { startMs = null, keep = 'finish' } = {}) {
  if (!original?.length || !rows.length || rows.length > 8) throw new Error('segment-limit');
  let cursor = 0;
  const spans = rows.map((row, index) => {
    const side = Number(row.side);
    if (![1, 2].includes(side)) throw new Error('invalid-side');
    if (index) cursor += parseDuration(row.pause, true);
    const span = { side, start: cursor, end: cursor + parseDuration(row.duration) };
    cursor = span.end;
    return span;
  });
  if (spans.reduce((sum, row) => sum + row.end - row.start, 0) > 240 * 60_000) throw new Error('duration-limit');
  const base = startMs ?? (keep === 'finish' ? original.at(-1).end_utc_ms - cursor : original[0].start_utc_ms);
  const rebuilt = spans.map((span, index) => {
    const start = base + span.start;
    const end = base + span.end;
    const previous = original[index];
    return {
      side: span.side,
      start_utc_ms: start,
      end_utc_ms: end,
      start_offset_minutes: start === previous?.start_utc_ms ? previous.start_offset_minutes : offsetAt(start),
      end_offset_minutes: end === previous?.end_utc_ms ? previous.end_offset_minutes : offsetAt(end),
    };
  });
  if (rebuilt.at(-1).end_utc_ms > Date.now()) throw new Error('future-end');
  return rebuilt;
}
