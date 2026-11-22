import assert from 'node:assert/strict';
import test from 'node:test';
const SIZE = 180; const STROKE = 22; const R = (SIZE - STROKE) / 2; const CIRC = 2 * Math.PI * R;
function slices(rows) {
  const total = rows.reduce((s, r) => s + r.value, 0);
  const sorted = rows.filter((r) => r.value > 0).sort((a, b) => b.value - a.value);
  const top = sorted.slice(0, 7); const tail = sorted.slice(7);
  const out = tail.length ? [...top, { name: 'Other', value: tail.reduce((s, r) => s + r.value, 0) }] : top;
  let offset = 0;
  return out.map((row) => { const share = total > 0 ? row.value / total : 0; const dash = share * CIRC; const s = { ...row, share, pct: Math.round(share * 1000) / 10, offset, dash }; offset += dash; return s; });
}
test('zero total produces no NaN geometry', () => {
  assert.equal(slices([{ name: 'k', value: 0 }]).length, 0);
  assert.ok(Number.isFinite(CIRC));
});
test('single key gets full 360 degree ring', () => {
  const out = slices([{ name: 'cloud', value: 3000 }]);
  assert.equal(out.length, 1);
  assert.equal(out[0].pct, 100);
  assert.ok(Math.abs(out[0].share * 360 - 360) < 1e-9);
  assert.ok(Number.isFinite(out[0].offset));
});
test('long tail is grouped into Other at 8th slice', () => {
  const rows = Array.from({ length: 9 }, (_, i) => ({ name: `k${i}`, value: 10 - i }));
  const out = slices(rows);
  assert.equal(out.length, 8);
  assert.equal(out[7].name, 'Other');
  assert.ok(Math.abs(out.reduce((s, r) => s + r.share, 0) - 1) < 1e-9);
});
