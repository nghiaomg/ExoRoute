/**
 * Rows shared by the statistics charts: the ranked bar list and its table twin
 * are built from the same array, so a row's identity, count and share are
 * decided here and cannot disagree between the two views.
 */
import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import { dimensionBars } from '../src/features/statistics/chartTypes';
import type { StatisticsDimension } from '../src/lib/types';

const tr = (key: string, vars: Record<string, string | number> = {}): string =>
  Object.entries(vars).reduce((text, [name, value]) => text.replace(`{${name}}`, String(value)), key);

const options = {
  tr,
  formatCount: (value: number): string => String(value),
  locale: 'en' as const,
  othersLabel: 'Others',
};

function dimension(id: string, requests: number, percentage: number): StatisticsDimension {
  return { id, label: `label-${id}`, requests, percentage };
}

test('dimension rows keep the server ranking and their own counts', () => {
  const rows = dimensionBars([dimension('a', 40, 40), dimension('b', 10, 10)], 0, 100, options);

  assert.deepEqual(
    rows.map((row) => row.label),
    ['label-a', 'label-b'],
  );
  assert.deepEqual(
    rows.map((row) => row.value),
    [40, 10],
  );
  assert.deepEqual(
    rows.map((row) => row.display),
    ['40', '10'],
  );
  assert.deepEqual(rows[0].detail, ['Requests: 40', 'Share: 40%']);
});

test('the aggregated remainder is appended as its own row with a unique id', () => {
  const rows = dimensionBars([dimension('a', 40, 40)], 60, 100, options);

  assert.equal(rows.length, 2);
  const others = rows[1];
  assert.equal(others.label, 'Others');
  assert.equal(others.value, 60);
  assert.deepEqual(others.detail, ['Requests: 60', 'Share: 60%']);
  assert.equal(new Set(rows.map((row) => row.id)).size, rows.length, 'row ids stay unique');
});

test('no remainder row is added when the list already covers the total', () => {
  const rows = dimensionBars([dimension('a', 40, 40)], 0, 100, options);

  assert.equal(rows.length, 1);
  assert.ok(!rows.some((row) => row.label === 'Others'));
});

test('a zero total reports a zero share instead of dividing by zero', () => {
  const rows = dimensionBars([], 7, 0, options);

  assert.equal(rows.length, 1);
  assert.deepEqual(rows[0].detail, ['Requests: 7', 'Share: 0%']);
});
