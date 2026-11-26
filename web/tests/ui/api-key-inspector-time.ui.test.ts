import { render } from '@testing-library/svelte';
import { describe, expect, test } from 'vitest';
import ApiKeyInspector from '../../src/features/api-keys/ApiKeyInspector.svelte';
import { EMPTY_METRICS } from '../../src/features/api-keys/api-keys.data';
import { t } from '../../src/lib/i18n';
import type { GatewayApiKey } from '../../src/lib/types';

const tr = (key: string, vars?: Record<string, string | number>) => t('vi', key, vars);
const fiveMinutesAgo = new Date(Date.now() - 5 * 60_000).toISOString().replace('T', ' ').replace('Z', '');
const apiKey: GatewayApiKey = {
  id: 'key-1',
  name: 'demo',
  enabled: true,
  created_at: '2026-01-01 00:00:00',
  last_used_at: fiveMinutesAgo,
};

describe('F-02 inspector relative time', () => {
  test('delegates to formatIdleLabel with a single localized suffix', () => {
    render(ApiKeyInspector, {
      props: { tr, locale: 'vi', apiKey, metrics: EMPTY_METRICS, onClose: () => {}, onNavigate: () => {} },
    });
    const text = document.body.textContent ?? '';
    expect(text).toContain('Đã dùng 5 phút trước');
    expect(text).not.toContain('trước trước');
    expect(text).not.toContain('phút trước trước');
  });
});
