import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import type { Provider } from '../src/lib/types';
import {
  addableScopeProviders,
  MAX_VISIBLE_SCOPE_PROVIDERS,
  modelRulesText,
  parseModelRules,
  toggleProviderScope,
} from '../src/features/api-keys/scope';

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

test('model rules are parsed one per line and trimmed', () => {
  assert.deepEqual(parseModelRules('  gpt-4*  \n\nclaude-3-5-sonnet\n   \n'), [
    'gpt-4*',
    'claude-3-5-sonnet',
  ]);
  assert.deepEqual(parseModelRules(''), []);
  assert.deepEqual(parseModelRules('   \n\t\n'), []);
});

test('stored model rules round-trip back into the editor text', () => {
  assert.equal(modelRulesText(['gpt-4*', 'coding']), 'gpt-4*\ncoding');
  assert.equal(modelRulesText([]), '');
  assert.equal(modelRulesText(null), '');
  assert.equal(modelRulesText(undefined), '');
});

test('provider scope toggling stays unique and ordered', () => {
  assert.deepEqual(toggleProviderScope([], 'provider-a', true), ['provider-a']);
  assert.deepEqual(toggleProviderScope(['provider-a'], 'provider-a', true), ['provider-a']);
  assert.deepEqual(
    toggleProviderScope(['provider-a', 'provider-b'], 'provider-b', false),
    ['provider-a'],
  );
  assert.deepEqual(toggleProviderScope(['provider-a'], 'missing', false), ['provider-a']);
});

test('the provider add-picker hides allowed providers and stays bounded', () => {
  const providers = [
    provider('openai', 'OpenAI'),
    provider('anthropic', 'Anthropic'),
    provider('local', 'Local Ollama'),
  ];
  const all = addableScopeProviders(providers, [], '');
  assert.deepEqual(
    all.options.map((entry) => entry.id),
    ['openai', 'anthropic', 'local'],
  );
  assert.equal(all.truncated, false);

  // A provider the key already allows cannot be added twice.
  assert.deepEqual(
    addableScopeProviders(providers, ['openai'], '').options.map((entry) => entry.id),
    ['anthropic', 'local'],
  );
  assert.deepEqual(
    addableScopeProviders(providers, ['openai', 'anthropic', 'local'], '').options,
    [],
  );

  const byName = addableScopeProviders(providers, [], 'oll');
  assert.deepEqual(
    byName.options.map((entry) => entry.id),
    ['local'],
  );
  const byId = addableScopeProviders(providers, [], 'OPENAI');
  assert.deepEqual(
    byId.options.map((entry) => entry.id),
    ['openai'],
  );
  assert.deepEqual(addableScopeProviders(providers, [], 'nothing').options, []);

  const many = Array.from({ length: MAX_VISIBLE_SCOPE_PROVIDERS + 5 }, (_, index) =>
    provider(`provider-${index}`, `Provider ${index}`),
  );
  const bounded = addableScopeProviders(many, [], '');
  assert.equal(bounded.options.length, MAX_VISIBLE_SCOPE_PROVIDERS);
  assert.equal(bounded.truncated, true);
  assert.equal(addableScopeProviders(many, ['provider-0'], '').truncated, true);
});
