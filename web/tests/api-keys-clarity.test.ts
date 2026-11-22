import assert from 'node:assert/strict';
import test from 'node:test';
import { formatIdleLabel, filterAndSortKeys, EMPTY_METRICS } from '../src/features/api-keys/api-keys.data';
const tr = (k: string, vars?: Record<string, string | number>) => k.replace(/\{(\w+)\}/g, (_, n) => String(vars?.[n] ?? `{${n}}`));
const base = { id: 'k1', name: 'cloud', enabled: true, created_at: '2026-01-01 00:00:00', base_url: '', auth_type: 'bearer', preferred_protocol: 'chat_completions', supported_protocols: [] as never };
test('relative labels never repeat the ago suffix', () => {
  const now = new Date('2026-11-17T12:00:00Z').valueOf();
  const oneMinute = { ...base, last_used_at: '2026-11-17 11:59:00' };
  const label = formatIdleLabel(oneMinute, (k, v) => k === 'Used {time} ago' ? `${v?.time} ago` : k === 'minutes' ? 'minutes' : k, now);
  assert.equal(label, '1 minutes ago');
  assert.ok(!label.includes('ago ago'));
});
test('status filter by erroring keys uses metrics, not enabled flag', () => {
  const keys = [base, { ...base, id: 'k2', name: 'idle' }];
  const metrics = { k1: { ...EMPTY_METRICS, loaded: true, requests: 3000, failures: 16 }, k2: { ...EMPTY_METRICS, loaded: true, requests: 100, failures: 0 } };
  const result = filterAndSortKeys(keys, { query: '', status: 'erroring', scope: 'all', sort: 'traffic' }, metrics);
  assert.deepEqual(result.map((k) => k.id), ['k1']);
  assert.equal(keys[0].enabled, true);
});
