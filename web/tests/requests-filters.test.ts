import { strict as assert } from 'node:assert/strict';
import test from 'node:test';

import {
  apiKeyFilterOption,
  providerFilterOption,
  rememberOption,
  withPinnedOption,
  type RequestFilterOption,
} from '../src/features/requests/request.filters';
import type { GatewayApiKey, Provider } from '../src/lib/types';

function apiKey(id: string, name: string): GatewayApiKey {
  return { id, name, enabled: true, created_at: '2026-10-05T10:00:00.000Z' };
}

function provider(id: string, name: string): Provider {
  return {
    id,
    name,
    adapter_id: 'generic',
    base_url: 'http://127.0.0.1:9/v1',
    enabled: true,
    auth_type: 'none',
    preferred_protocol: 'chat_completions',
    supported_protocols: ['chat_completions'],
  };
}

test('the id fields read by name and search by id', () => {
  assert.deepEqual(apiKeyFilterOption(apiKey('key-1', 'Production key')), {
    label: 'Production key',
    value: 'key-1',
    hint: 'key-1',
    keywords: 'key-1',
  });
  assert.deepEqual(providerFilterOption(provider('p1', 'OpenAI')), {
    label: 'OpenAI',
    value: 'p1',
    hint: 'p1',
    keywords: 'p1',
  });
});

test('a selection stays listed after a later search replaced the options', () => {
  const loaded: RequestFilterOption[] = [{ label: 'OpenAI', value: 'p1', hint: 'p1' }];

  // Nothing selected, or the value is already listed: the list is untouched.
  assert.equal(withPinnedOption(loaded, '', ''), loaded);
  assert.equal(withPinnedOption(loaded, 'p1', 'OpenAI'), loaded);

  assert.deepEqual(withPinnedOption(loaded, 'p9', 'Retired provider'), [
    { label: 'Retired provider', value: 'p9', hint: 'p9' },
    { label: 'OpenAI', value: 'p1', hint: 'p1' },
  ]);
  // Without a remembered name the id stands in, so the field never blanks.
  assert.deepEqual(withPinnedOption([], 'p9', ''), [{ label: 'p9', value: 'p9', hint: 'p9' }]);
});

test('remembered option names are bounded', () => {
  let remembered = new Map<string, RequestFilterOption>();
  for (let index = 0; index < 5; index += 1) {
    remembered = rememberOption(remembered, { label: `Key ${index}`, value: `key-${index}` }, 3);
  }

  assert.equal(remembered.size, 3);
  assert.deepEqual([...remembered.keys()], ['key-2', 'key-3', 'key-4']);

  // Re-remembering moves an entry to the end instead of duplicating it.
  remembered = rememberOption(remembered, { label: 'Key 3 renamed', value: 'key-3' }, 3);
  assert.equal(remembered.size, 3);
  assert.equal(remembered.get('key-3')?.label, 'Key 3 renamed');
  assert.deepEqual([...remembered.keys()], ['key-2', 'key-4', 'key-3']);
});
