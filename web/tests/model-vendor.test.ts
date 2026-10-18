import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import { pickModelVendor } from '../src/features/requests/model-vendor';

// The stems the dashboard actually ships. A test asserts against this set
// instead of the folder so the mapping stays checkable without Vite.
const shipped = new Set([
  'claude',
  'deepseek',
  'gemini',
  'grok',
  'inclusion',
  'kimi',
  'meta',
  'minimax',
  'mistral',
  'openai',
  'poolside',
  'qwen',
  'sakana',
  'tencenthy',
  'xiaomi',
  'z-ai',
]);

test('model families resolve to their vendor file', () => {
  const cases: ReadonlyArray<readonly [string, string]> = [
    ['claude-opus-4-6', 'claude'],
    ['claude-sonnet-5', 'claude'],
    ['gpt-5.4', 'openai'],
    ['gpt-6-astra', 'openai'],
    ['codex-mini-latest', 'openai'],
    ['o3-mini', 'openai'],
    ['gemini-3.7-flash', 'gemini'],
    ['grok-4.7', 'grok'],
    ['deepseek-v4-pro', 'deepseek'],
    ['kimi-k2.6', 'kimi'],
    ['minimax-m3', 'minimax'],
    ['mimo-v2.6-flash', 'xiaomi'],
    ['glm-5.3-flash', 'z-ai'],
    ['qwen3.8-max', 'qwen'],
    ['qwen3.6-plus', 'qwen'],
    ['llama-4-maverick', 'meta'],
    ['mistral-large-3', 'mistral'],
    ['devstral-small', 'mistral'],
    ['hy4-preview', 'tencenthy'],
    ['hy3', 'tencenthy'],
    ['hunyuan-turbo', 'tencenthy'],
    ['laguna-xs.2', 'poolside'],
    ['fugu-ultra', 'sakana'],
    ['ling-3.0-flash-fin-free', 'inclusion'],
    ['ring-lite', 'inclusion'],
  ];
  for (const [model, vendor] of cases) {
    assert.equal(pickModelVendor(model, shipped), vendor, model);
  }
});

test('a provider prefix decides before the model family', () => {
  assert.equal(pickModelVendor('ocg/minimax-m3', shipped), 'minimax');
  assert.equal(pickModelVendor('openai/gpt-5', shipped), 'openai');
  assert.equal(pickModelVendor('anthropic/claude', shipped), 'claude');
  assert.equal(pickModelVendor('x-ai/grok-4', shipped), 'grok');
  // The prefix wins when it names a shipped vendor itself.
  assert.equal(pickModelVendor('openai/deepseek-v4-pro', shipped), 'openai');
});

test('an unknown family keeps the monogram fallback', () => {
  for (const model of [
    'live/preview-model',
    'muse-spark-1.3',
    'nemotron-3-ultra-free',
    'space-bunny-free',
    'longcat-2.0',
    'brand-new-model',
    '',
  ]) {
    assert.equal(pickModelVendor(model, shipped), null, model);
  }
  assert.equal(pickModelVendor(null, shipped), null);
  assert.equal(pickModelVendor(undefined, shipped), null);
});

test('a newly shipped vendor covers its own model ids without a mapping', () => {
  // `nemotron.svg` is not part of the alias table; adding the file is enough.
  assert.equal(pickModelVendor('nemotron-3-ultra-free', new Set(['nemotron'])), 'nemotron');
  assert.equal(pickModelVendor('nvidia/nemotron-4', new Set(['nvidia'])), 'nvidia');
  // A file that ships is only used when its own name appears in the id.
  assert.equal(pickModelVendor('muse-spark-1.3', new Set(['nemotron'])), null);
});

test('matching ignores case and separators', () => {
  assert.equal(pickModelVendor('DeepSeek-V4-Pro', shipped), 'deepseek');
  assert.equal(pickModelVendor('Kimi_K2.6', shipped), 'kimi');
  assert.equal(pickModelVendor('  glm-5.3  ', shipped), 'z-ai');
});
