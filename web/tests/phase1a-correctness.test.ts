import assert from 'node:assert/strict';
import test from 'node:test';
import { providerHealth } from '../src/features/overview/overview.data';
import { formatIdleLabel } from '../src/features/api-keys/api-keys.data';
import { catalogs, t } from '../src/lib/i18n';
import type { GatewayApiKey, Provider, ProviderQuotaResult } from '../src/lib/types';

const provider = (id: string, enabled: boolean, invalid = 0): Provider => ({
  id,
  adapter_id: 'openai',
  name: id,
  base_url: '',
  enabled,
  auth_type: 'bearer',
  invalid_api_key_count: invalid,
  preferred_protocol: 'chat_completions',
  supported_protocols: ['chat_completions'],
});

const quota = (provider_id: string, remaining_percent: number): ProviderQuotaResult => ({
  provider_id,
  window_seconds: 60,
  requests_last_60_seconds: 0,
  target_rpm: 60,
  remaining_percent,
  used_percent: 100 - remaining_percent,
  coverage_seconds: 60,
  sampled_at_ms: 0,
  status: remaining_percent <= 0 ? 'capacity_limited' : 'ready',
  source: 'exoroute_local',
  nvidia_source: 'not_documented',
});

test('F-01: a disabled first provider does not shift quota onto later providers', () => {
  const providers = [provider('disabled', false), provider('healthy', true), provider('exhausted', true)];
  // Only 'exhausted' has a quota; positional lookup would apply it to 'healthy'.
  const quotas = new Map([['exhausted', quota('exhausted', 0)]]);
  const health = providerHealth(providers, quotas);
  assert.deepEqual(health, { total: 3, active: 1, degraded: 1, disabled: 1, invalidKeys: 0 });
});

test('F-01: quotas beyond the 12-provider fetch window never shift onto another provider', () => {
  const providers = Array.from({ length: 13 }, (_, i) => provider('p' + i, true));
  // The 13th provider is outside the fetched subset in DashboardOverviewPage.
  const quotas = new Map([['p12', quota('p12', 0)]]);
  const health = providerHealth(providers, quotas);
  assert.equal(health.active, 12);
  assert.equal(health.degraded, 1);
  assert.equal(health.disabled, 0);
  // A quota for an unknown provider id must be ignored entirely.
  const stray = providerHealth(providers, new Map([['unknown', quota('unknown', 0)]]));
  assert.equal(stray.active, 13);
  assert.equal(stray.degraded, 0);
});

const keyWithAge = (minutesAgo: number, now: number): GatewayApiKey => ({
  id: 'k1',
  name: 'demo',
  enabled: true,
  created_at: '2026-01-01 00:00:00',
  last_used_at: new Date(now - minutesAgo * 60_000).toISOString().replace('T', ' ').replace('Z', ''),
});

test('F-02/F-03: relative labels have exactly one suffix in en and vi', () => {
  const now = new Date('2026-11-17T12:00:00Z').valueOf();
  const tr = (locale: 'en' | 'vi') => (k: string, vars?: Record<string, string | number>) => t(locale, k, vars);
  for (const [minutesAgo, word] of [[0, 'just now'], [5, 'minutes'], [120, 'hours'], [2880, 'days']] as const) {
    const key = keyWithAge(minutesAgo, now);
    const en = formatIdleLabel(key, tr('en'), now);
    const vi = formatIdleLabel(key, tr('vi'), now);
    assert.ok(!en.includes('ago ago'), en);
    assert.ok(!vi.includes('trước trước'), vi);
    if (minutesAgo === 0) {
      assert.equal(en, 'just now');
      assert.equal(vi, 'vừa xong');
      continue;
    }
    assert.ok(en.includes(word), en);
    assert.ok(!en.includes('{minutes}') && !en.includes('{hours}'), en);
    assert.ok(!vi.includes('minutes') && !vi.includes('hours'), vi);
    assert.ok(vi.includes('trước'), vi);
  }
  const never = formatIdleLabel({ ...keyWithAge(5, now), last_used_at: null }, tr('vi'), now);
  assert.equal(never, 'Chưa từng dùng');
});

test('F-03: minute, hour and day unit keys resolve in all twelve catalogs', () => {
  for (const locale of ['en','vi','zh','es','pt','ja','id','ko','de','fr','hi','ar'] as const) {
    for (const key of ['minutes', 'hours', 'days'] as const) {
      const catalog = catalogs[locale] as Record<string, string>;
      assert.ok(Object.hasOwn(catalog, key), locale + ' missing key ' + key);
      const value = t(locale, key);
      assert.ok(value.trim().length > 0);
    }
  }
});

test('F-04: key counts use localized parameterized keys for 0, 1 and N keys', () => {
  const fmt = (locale: 'en' | 'vi', n: number) => t(locale, n === 1 ? '{count} key' : '{count} keys', { count: n });
  assert.equal(fmt('en', 0), '0 keys');
  assert.equal(fmt('en', 1), '1 key');
  assert.equal(fmt('en', 2), '2 keys');
  assert.equal(fmt('vi', 0), '0 key');
  assert.equal(fmt('vi', 1), '1 key');
  assert.equal(fmt('vi', 2), '2 key');
  for (const locale of ['zh','es','pt','ja','id','ko','de','fr','hi','ar'] as const) {
    const singular = t(locale, '{count} key', { count: 1 });
    const plural = t(locale, '{count} keys', { count: 2 });
    assert.ok(!singular.includes('{count}') && !plural.includes('{count}'), locale);
    assert.ok(singular.includes('1') && plural.includes('2'), locale);
  }
});
