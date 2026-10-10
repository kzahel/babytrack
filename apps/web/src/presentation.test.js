import assert from 'node:assert/strict';
import test from 'node:test';
import { canonicalDecimal, clock, dayWindow, durationLabel, elapsedLabel, entrySummary,
  plannedSegments, touchesDay, validDecimal } from './presentation.js';

test('entry summaries follow the Android history lines', () => {
  assert.equal(entrySummary({ kind: 'feed.bottle', bottleEntered: '4.5', bottleUnit: 2, bottleContent: 2 }),
    'Bottle · 4.5 US fl oz · Formula');
  assert.equal(entrySummary({ kind: 'feed.bottle', bottleMl: 90, bottleContent: 1 }), 'Bottle · 90 mL · Breast milk');
  assert.equal(entrySummary({ kind: 'feed.breast', startMs: 0, breastSegments: [
    { side: 1, start_utc_ms: 0, end_utc_ms: 300_000 }, { side: 2, start_utc_ms: 300_000, end_utc_ms: 720_000 }] }),
  'Breast · Left 5 min → Right 7 min');
  assert.equal(entrySummary({ kind: 'feed.breast', startMs: 0, breastSegments: [
    { side: 2, start_utc_ms: 0, end_utc_ms: 600_000 }] }), 'Breast · Right · 10 min');
  assert.equal(entrySummary({ kind: 'pump', startMs: 0, endMs: 900_000, pumpTotalMl: 90 }), 'Pump · 90 mL total · 15 min');
  assert.equal(entrySummary({ kind: 'pump', startMs: 0, endMs: 600_000, pumpLeftMl: 40, pumpRightMl: 35 }),
    'Pump · L 40 mL · R 35 mL · 10 min');
  assert.equal(entrySummary({ kind: 'feed.solids', solidsFoods: ['pear', 'oats'], solidsAmount: 'some' }),
    'Solids · pear, oats · some');
  assert.equal(entrySummary({ kind: 'sleep', startMs: 0 }), 'Sleep · running');
  assert.equal(entrySummary({ kind: 'sleep', startMs: 0, endMs: 5_400_000 }), 'Sleep · 1 h 30 min');
  assert.equal(entrySummary({ kind: 'growth', growthWeightEntered: '5.25', growthWeightUnit: 11,
    growthLengthEntered: '58', growthLengthUnit: 21, growthHeadEntered: '38', growthHeadUnit: 21 }),
  'Growth · 5.25 kg · 58 cm · head 38 cm');
  assert.equal(entrySummary({ kind: 'temperature', temperatureEntered: '98.6', temperatureUnit: 31 }),
    'Temperature · 98.6 °F');
  assert.equal(entrySummary({ kind: 'medication', medicationName: 'Iron', medicationDoseAmount: '2',
    medicationDoseUnit: 'ml' }), 'Medication · Iron · 2 ml');
  assert.equal(entrySummary({ kind: 'diaper', diaperKind: 3 }), 'Diaper · Both');
  assert.equal(entrySummary({ kind: 'note', note: 'Hi' }), 'Note · Hi');
  assert.equal(entrySummary({ kind: 'future.kind' }), 'Entry from a newer app version');
});

test('durations, elapsed labels, and clocks', () => {
  assert.equal(durationLabel(45 * 60_000), '45 min');
  assert.equal(durationLabel(120 * 60_000), '2 h');
  assert.equal(elapsedLabel(20_000), 'Just now');
  assert.equal(elapsedLabel(65 * 60_000), '1 h 5 min ago');
  assert.equal(elapsedLabel(49 * 3_600_000), '2 days ago');
  assert.equal(clock(65_000), '1:05');
  assert.equal(clock(3_725_000), '1:02:05');
});

test('decimal validation matches the Android amount rule', () => {
  assert.equal(canonicalDecimal(' 4,5 '), '4.5');
  assert.ok(validDecimal('4,5'));
  assert.ok(!validDecimal('4.5', { integer: true }));
  assert.ok(!validDecimal('0'));
  assert.ok(!validDecimal('01'));
  assert.ok(validDecimal('-0.5', { signed: true }));
  assert.ok(!validDecimal('1000001', { max: 1_000_000 }));
});

test('day windows and interval days', () => {
  const noon = new Date(2026, 9, 10, 12).getTime();
  const window = dayWindow(noon, noon);
  assert.equal(window.startMs, new Date(2026, 9, 10).getTime());
  assert.equal(window.endMs, new Date(2026, 9, 11).getTime());
  assert.equal(window.throughMs, noon);
  const overnight = { kind: 'sleep', startMs: window.startMs - 3_600_000, endMs: window.startMs + 3_600_000 };
  assert.ok(touchesDay(overnight, window, noon));
  assert.ok(!touchesDay({ kind: 'diaper', startMs: window.endMs }, window, noon));
});

test('manual breast minutes end at the chosen time', () => {
  const end = 10_000_000;
  const segments = plannedSegments([{ side: 1, minutes: 5 }, { side: 2, minutes: 7 }], end);
  assert.equal(segments[0].start_utc_ms, end - 12 * 60_000);
  assert.equal(segments[0].end_utc_ms, segments[1].start_utc_ms);
  assert.equal(segments[1].end_utc_ms, end);
});

test('Today since-last values and the day ribbon', async () => {
  const { shortElapsed, feedDetail, nextBreastSide, ribbon } = await import('./presentation.js');
  assert.equal(shortElapsed(45 * 60_000), '45m');
  assert.equal(shortElapsed(135 * 60_000), '2h 15m');
  assert.equal(shortElapsed(65 * 60_000), '1h 05m');
  assert.equal(shortElapsed(72 * 3_600_000), '3d');
  assert.equal(feedDetail({ kind: 'feed.bottle', bottleEntered: '120', bottleUnit: 1 }), '120 mL bottle');
  assert.equal(feedDetail({ kind: 'feed.breast', breastSegments: [{ side: 1 }, { side: 2 }] }), 'Breast · Right side');
  assert.equal(nextBreastSide([{ kind: 'feed.breast', startMs: 1, breastSegments: [{ side: 1 }] }]), 2);
  assert.equal(nextBreastSide([]), null);
  const window = { startMs: 0, endMs: 100_000, throughMs: 50_000 };
  const layout = ribbon([
    { kind: 'sleep', startMs: -10_000, endMs: 20_000 },
    { kind: 'sleep', startMs: 40_000 },
    { kind: 'feed.bottle', startMs: 30_000 },
    { kind: 'diaper', startMs: 60_000 },
  ], window, 50_000);
  assert.deepEqual(layout.sleeps, [{ left: 0, width: 20 }, { left: 40, width: 10 }]);
  assert.deepEqual(layout.feeds, [30]);
  assert.deepEqual(layout.diapers, [], 'a future diaper is not drawn');
  assert.equal(layout.now, 50);
});
