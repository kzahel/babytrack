import { copy as c } from './strings.js';

// Published codes, matching the Android forms and the records contract.
export const diaperKinds = { 1: c.wet, 2: c.dirty, 3: c.both, 4: c.dry };
export const bottleContents = { 1: c.milk, 2: c.formula, 3: c.mixed, 4: c.otherMilk };
export const bottleUnits = { 1: 'mL', 2: 'US fl oz', 3: 'UK fl oz' };
export const weightUnits = { 10: 'g', 11: 'kg', 12: 'lb', 13: 'oz' };
export const lengthUnits = { 20: 'mm', 21: 'cm', 22: 'in' };
export const temperatureUnits = { 30: '°C', 31: '°F' };
export const sleepPlaces = { 1: c.placeCrib, 2: c.placePram, 3: c.placeContact, 4: c.placeCar, 5: c.placeOther };
export const sexes = { 1: c.female, 2: c.male, 3: c.unspecified };

export const chooser = [
  { title: c.groupFeeding, kinds: ['feed.bottle', 'feed.breast', 'pump', 'feed.solids'] },
  { title: c.groupSleepDiapers, kinds: ['sleep', 'diaper'] },
  { title: c.groupHealth, kinds: ['growth', 'temperature', 'medication'] },
  { title: c.groupNotes, kinds: ['note'] },
];

export const kindLabels = {
  'feed.bottle': c.bottle, 'feed.breast': c.breastFeed, pump: c.pumping, 'feed.solids': c.solids,
  sleep: c.sleep, diaper: c.diaper, growth: c.growth, temperature: c.temperature,
  medication: c.medication, note: c.note,
};

const categories = {
  'feed.bottle': 'feed', 'feed.breast': 'feed', 'feed.solids': 'feed', pump: 'feed', sleep: 'sleep',
  diaper: 'care', growth: 'health', temperature: 'health', medication: 'health', note: 'note',
};
const icons = {
  'feed.bottle': 'bottle', 'feed.breast': 'breast', pump: 'pump', 'feed.solids': 'solids', sleep: 'sleep',
  diaper: 'diaper', growth: 'growth', temperature: 'temperature', medication: 'medication', note: 'note',
};
export const category = (kind) => categories[kind] || 'note';
/** The Icon name for an activity kind. */
export const iconFor = (kind) => icons[kind] || 'note';

export const filters = [
  { id: 'all', label: c.filterAll, match: () => true },
  { id: 'feeds', label: c.feeds, match: (kind) => kind.startsWith('feed.') || kind === 'pump' },
  { id: 'sleep', label: c.sleep, match: (kind) => kind === 'sleep' },
  { id: 'diapers', label: c.diapers, match: (kind) => kind === 'diaper' },
  { id: 'care', label: c.filterCare, match: (kind) => ['growth', 'temperature', 'medication'].includes(kind) },
  { id: 'notes', label: c.filterNotes, match: (kind) => kind === 'note' },
];

const number = (value) => new Intl.NumberFormat().format(value);
const decimalSeparator = () => new Intl.NumberFormat().formatToParts(1.5)
  .find((part) => part.type === 'decimal')?.value || '.';

/** Show an entered decimal with the viewer's separator; storage keeps '.'. */
export const localized = (entered) => String(entered).replace('.', decimalSeparator());

/** Accept '.' or ',' as typed; the core receives a canonical '.' decimal. */
export const canonicalDecimal = (value) => String(value).trim().replace(',', '.');

/** The Android amount rule: plain digits, one nonzero digit, optional fraction. */
export function validDecimal(value, { integer = false, signed = false, max } = {}) {
  const text = canonicalDecimal(value);
  const pattern = signed ? /^-?(0|[1-9][0-9]*)(\.[0-9]+)?$/ : /^(0|[1-9][0-9]*)(\.[0-9]+)?$/;
  if (!text || text.length > 16 || !pattern.test(text)) return false;
  if (integer && text.includes('.')) return false;
  if (!signed && !/[1-9]/.test(text)) return false;
  return max === undefined || Number(text) <= max;
}

export const minutes = (ms) => Math.max(0, Math.round(ms / 60_000));

export function durationLabel(ms) {
  const total = minutes(ms);
  const hours = Math.floor(total / 60);
  if (!hours) return c.minutesShort(total);
  return total % 60 ? c.hoursMinutesShort(hours, total % 60) : c.hoursShort(hours);
}

export function elapsedLabel(ms) {
  const total = Math.floor(Math.max(0, ms) / 60_000);
  if (total < 1) return c.justNow;
  if (total < 60) return c.minutesAgo(total);
  const hours = Math.floor(total / 60);
  if (hours < 24) return total % 60 ? c.hoursMinutesAgo(hours, total % 60) : c.hoursAgo(hours);
  return c.daysAgo(Math.floor(hours / 24));
}

/** A running timer: m:ss below an hour, then h:mm:ss. */
export function clock(ms) {
  const seconds = Math.floor(Math.max(0, ms) / 1000);
  const two = (value) => String(value).padStart(2, '0');
  const hours = Math.floor(seconds / 3600);
  return hours ? `${hours}:${two(Math.floor(seconds / 60) % 60)}:${two(seconds % 60)}`
    : `${Math.floor(seconds / 60)}:${two(seconds % 60)}`;
}

export const clockTime = (ms) => new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(ms);

export function compactDateTime(ms, now = Date.now()) {
  const day = startOfDay(ms);
  const today = startOfDay(now);
  if (day === today) return clockTime(ms);
  if (day === startOfDay(today - 1)) return c.yesterdayAt(clockTime(ms));
  const sameYear = new Date(ms).getFullYear() === new Date(now).getFullYear();
  const date = new Intl.DateTimeFormat(undefined, sameYear ? { month: 'short', day: 'numeric' }
    : { year: 'numeric', month: 'short', day: 'numeric' }).format(ms);
  return c.dateAt(date, clockTime(ms));
}

export function dayHeading(ms, now = Date.now()) {
  const day = startOfDay(ms);
  if (day === startOfDay(now)) return c.today;
  if (day === startOfDay(startOfDay(now) - 1)) return c.yesterday;
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'full' }).format(ms);
}

/** Local midnight for the day containing `ms`, honoring DST transitions. */
export function startOfDay(ms) {
  const date = new Date(ms);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/** The viewer's local day window, as the core day summary expects. */
export function dayWindow(ms, now = Date.now()) {
  const start = startOfDay(ms);
  const date = new Date(start);
  const end = new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1).getTime();
  return { startMs: start, endMs: end, throughMs: Math.min(now, end) };
}

/** Whether an entry belongs to a day; intervals appear on each day they touch. */
export function touchesDay(row, window, now = Date.now()) {
  const end = row.endMs ?? (row.kind === 'sleep' ? now : row.startMs);
  return row.startMs < window.endMs && Math.max(end, row.startMs) >= window.startMs;
}

export const isRunningSleep = (row) => row.kind === 'sleep' && row.endMs == null;

const measure = (entered, unit, units, base, baseUnit) => entered != null && units[unit]
  ? `${localized(entered)} ${units[unit]}` : base != null ? `${number(base)} ${baseUnit}` : null;

function breastSummary(row) {
  const segments = row.breastSegments || [];
  const side = (value) => (value === 1 ? c.left : c.right);
  // Android saves whole minutes; a browser timer keeps seconds, shown as m:ss.
  const length = (ms) => (ms % 60_000 ? clock(ms) : durationLabel(ms));
  if (segments.length <= 1) {
    const only = segments[0];
    const ms = only ? only.end_utc_ms - only.start_utc_ms : (row.endMs ?? row.startMs) - row.startMs;
    return c.entry(c.breast, side(only?.side ?? row.breastSide), length(ms));
  }
  return c.entry(c.breast, segments.map((item) =>
    `${side(item.side)} ${length(item.end_utc_ms - item.start_utc_ms)}`).join(' → '));
}

/** One line per entry, matching the Android history summaries. */
export function entrySummary(row) {
  switch (row.kind) {
    case 'feed.bottle': return c.entry(c.bottle,
      measure(row.bottleEntered, row.bottleUnit, bottleUnits, row.bottleMl, 'mL'),
      bottleContents[row.bottleContent]);
    case 'feed.breast': return breastSummary(row);
    case 'pump': {
      const duration = durationLabel((row.endMs ?? row.startMs) - row.startMs);
      if (row.pumpTotalMl != null) return c.entry(c.pump, c.totalMl(number(row.pumpTotalMl)), duration);
      return c.entry(c.pump, row.pumpLeftMl != null ? c.leftMl(number(row.pumpLeftMl)) : null,
        row.pumpRightMl != null ? c.rightMl(number(row.pumpRightMl)) : null, duration);
    }
    case 'feed.solids': return c.entry(c.solids, (row.solidsFoods || []).join(', '), row.solidsAmount);
    case 'sleep': return isRunningSleep(row) ? c.sleepRunning
      : c.entry(c.sleep, durationLabel(row.endMs - row.startMs));
    case 'note': return c.entry(c.note, row.note);
    case 'growth': return c.entry(c.growth,
      measure(row.growthWeightEntered, row.growthWeightUnit, weightUnits, row.growthWeightG, 'g'),
      measure(row.growthLengthEntered, row.growthLengthUnit, lengthUnits, row.growthLengthMm, 'mm'),
      row.growthHeadMm != null || row.growthHeadEntered != null
        ? c.head(measure(row.growthHeadEntered, row.growthHeadUnit, lengthUnits, row.growthHeadMm, 'mm')) : null);
    case 'temperature': return c.entry(c.temperature,
      row.temperatureEntered != null && temperatureUnits[row.temperatureUnit]
        ? `${localized(row.temperatureEntered)} ${temperatureUnits[row.temperatureUnit]}`
        : row.temperatureC != null ? `${localized(row.temperatureC)} °C` : null);
    case 'medication': return c.entry(c.medication, row.medicationName,
      [row.medicationDoseAmount, row.medicationDoseUnit].filter(Boolean).join(' '));
    case 'diaper': return c.entry(c.diaper, diaperKinds[row.diaperKind]);
    default: return c.newerEntry;
  }
}

/** The newest matching entry, by start. */
export const latest = (rows, match) => rows.filter((row) => match(row.kind))
  .reduce((best, row) => (!best || row.startMs > best.startMs ? row : best), null);

/** Lay manual breast minutes back to back so the last side ends at `endMs`. */
export function plannedSegments(sides, endMs) {
  let start = endMs - sides.reduce((sum, item) => sum + item.minutes, 0) * 60_000;
  return sides.map((item) => {
    const end = start + item.minutes * 60_000;
    const segment = { side: item.side, start_utc_ms: start, end_utc_ms: end,
      start_offset_minutes: -new Date(start).getTimezoneOffset(),
      end_offset_minutes: -new Date(end).getTimezoneOffset() };
    start = end;
    return segment;
  });
}

/** Epoch day for a yyyy-mm-dd date input, and back. */
export const epochDay = (value) => (value ? Math.floor(Date.parse(`${value}T12:00:00Z`) / 86_400_000) : null);
export const dateInput = (day) => (day == null ? '' : new Date(day * 86_400_000).toISOString().slice(0, 10));

/** A datetime-local input value for an instant, and back. */
export function localInput(ms) {
  const date = new Date(ms - new Date(ms).getTimezoneOffset() * 60_000);
  return date.toISOString().slice(0, 16);
}
export const fromLocalInput = (value) => new Date(value).getTime();

/** "2h 15m", "45m", "3d": the large number on Today's since-last surfaces. */
export function shortElapsed(ms) {
  const minutes = Math.max(0, Math.floor(ms / 60_000));
  if (minutes < 60) return c.shortMinutes(minutes);
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return c.shortHoursMinutes(hours, String(minutes % 60).padStart(2, '0'));
  return c.shortDays(Math.floor(hours / 24));
}

/** One line under the last feed: what it was. */
export function feedDetail(row) {
  if (row.kind === 'feed.bottle') {
    return c.bottleDetail(measure(row.bottleEntered, row.bottleUnit, bottleUnits, row.bottleMl, 'mL'));
  }
  if (row.kind === 'feed.breast') {
    const side = row.breastSegments?.at(-1)?.side ?? row.breastSide;
    return side ? c.breastSideDetail(side === 1 ? c.left : c.right) : c.breast;
  }
  return kindLabels[row.kind] || c.feeds;
}

/** The side to offer next: the one the last breast feed did not end on. */
export function nextBreastSide(rows) {
  const last = latest(rows, (kind) => kind === 'feed.breast');
  const side = last?.breastSegments?.at(-1)?.side ?? last?.breastSide;
  return side === 1 ? 2 : side === 2 ? 1 : null;
}

/**
 * Positions on the day ribbon as percentages of the local day: sleep spans
 * clipped to the day and the present, feed and diaper instants, and now.
 */
export function ribbon(rows, window, now = Date.now()) {
  const span = window.endMs - window.startMs;
  const at = (ms) => Math.min(100, Math.max(0, ((ms - window.startMs) / span) * 100));
  const through = Math.min(now, window.endMs);
  const inDay = (ms) => ms >= window.startMs && ms < window.endMs && ms <= through;
  return {
    sleeps: rows.filter((row) => row.kind === 'sleep').map((row) => {
      const from = Math.max(row.startMs, window.startMs);
      const to = Math.min(row.endMs ?? through, through);
      return to > from ? { left: at(from), width: at(to) - at(from) } : null;
    }).filter(Boolean),
    feeds: rows.filter((row) => (row.kind.startsWith('feed.') || row.kind === 'pump') && inDay(row.startMs)).map((row) => at(row.startMs)),
    diapers: rows.filter((row) => row.kind === 'diaper' && inDay(row.startMs)).map((row) => at(row.startMs)),
    now: at(through),
  };
}
