import { strict as assert } from 'node:assert/strict';
import test from 'node:test';

interface ProviderPresetLike {
  id: string;
  adapter_id: string;
}

const VALID_PANELS = new Set(['openai_codex', 'command_code', 'cline']);

export function resolveAuthPanel(
  authPanel: string | null | undefined,
  registry: Record<string, string>,
): string | null {
  if (!authPanel) return null;
  if (Object.hasOwn == null) return registry[authPanel] ?? null;
  return Object.hasOwn(registry, authPanel) ? registry[authPanel] : null;
}

test('unknown auth panel names never resolve to an unrelated panel', () => {
  const registry: Record<string, string> = {
    openai_codex: 'CodexOAuthPanel',
    command_code: 'CommandCodeAuthAssistPanel',
    cline: 'CodexOAuthPanel',
  };
  assert.equal(resolveAuthPanel('unknown_panel', registry), null);
  assert.equal(resolveAuthPanel(null, registry), null);
  assert.equal(resolveAuthPanel(undefined, registry), null);
  assert.equal(resolveAuthPanel('toString', registry), null);
  assert.equal(resolveAuthPanel('openai_codex', registry), 'CodexOAuthPanel');
});

test('preset adapter contract requires identical capabilities per adapter', () => {
  const presets: Array<ProviderPresetLike & { capabilities: string; auth: string }> = [
    { id: 'cline', adapter_id: 'cline', capabilities: 'oauth', auth: 'codex_oauth' },
    { id: 'clinepass', adapter_id: 'cline', capabilities: 'oauth', auth: 'codex_oauth' },
  ];
  const byAdapter = new Map<string, Array<(typeof presets)[number]>>();
  for (const preset of presets) {
    const group = byAdapter.get(preset.adapter_id) ?? [];
    group.push(preset);
    byAdapter.set(preset.adapter_id, group);
  }
  for (const [, group] of byAdapter) {
    const first = group[0];
    assert.ok(group.every((item) => item.capabilities === first.capabilities));
    assert.ok(group.every((item) => item.auth === first.auth));
  }
});

test('supported auth panel names stay within the build-time whitelist', () => {
  const panels: Array<string | null> = ['openai_codex', 'command_code', 'cline', null];
  for (const panel of panels) {
    assert.ok(panel === null || VALID_PANELS.has(panel), `unexpected panel ${panel}`);
  }
});

test('Antigravity provider i18n keys exist across all supported locales', async () => {
  const { catalogs } = await import('../src/lib/i18n');

  const antigravityKeys = [
    'Antigravity',
    'Sign in with a Google account and route Gemini, Claude, and open models through Google Cloud Code.',
    'e.g. ag/model-id',
    'Antigravity OAuth',
    'Antigravity account must be reconnected',
    'Antigravity account was rejected by Google; reconnect the account',
    'Antigravity account has no Cloud Code project; reconnect the account',
    'Antigravity account has no Cloud Code project; reconnect the account after completing Gemini Code Assist onboarding',
    'Antigravity quota service is temporarily unavailable',
    'Antigravity returned no supported models',
    'No Antigravity accounts connected yet.',
    'Antigravity endpoint is invalid',
    'Antigravity OAuth account is unavailable',
    'Antigravity runtime service is unavailable',
    'Gemini · Weekly',
    'Claude & GPT · Weekly',
    'Gemini Models',
    'Weekly Limit',
    'Google user info',
    'Google account has no Cloud Code project; complete Gemini Code Assist onboarding and reconnect Antigravity',
    'could not load the Antigravity account',
    'could not save the Antigravity account',
    'Antigravity account was removed, disabled, or needs reconnecting',
  ];

  for (const [lang, cat] of Object.entries(catalogs)) {
    for (const key of antigravityKeys) {
      assert.ok(key in cat, `Antigravity key "${key}" missing in ${lang}`);
      const val = (cat as Record<string, string>)[key];
      assert.ok(typeof val === 'string' && val.length > 0, `Antigravity key "${key}" in ${lang} is empty`);
    }
  }
});

test('Request error dialog i18n keys exist across all supported locales', async () => {
  const { catalogs } = await import('../src/lib/i18n');

  const errorDialogKeys = [
    'Request error details',
    'Copy error',
    'Error message',
    'REQUEST DIAGNOSTICS',
  ];

  for (const [lang, cat] of Object.entries(catalogs)) {
    for (const key of errorDialogKeys) {
      assert.ok(key in cat, `Error dialog key "${key}" missing in ${lang}`);
      const val = (cat as Record<string, string>)[key];
      assert.ok(typeof val === 'string' && val.length > 0, `Error dialog key "${key}" in ${lang} is empty`);
    }
  }
});

test('OAuth dialog step i18n keys exist across all supported locales', async () => {
  const { catalogs } = await import('../src/lib/i18n');

  const oauthDialogKeys = [
    'OAuth steps',
    'Step 1: Open this URL in your browser',
    'Open authorization page',
    'Step 2: Paste callback URL or authorization code here',
    'Paste callback URL or authorization code',
    'Paste OAuth callback URL or authorization code',
    'Paste the callback URL or authorization code',
    'OAuth callback URL or authorization code',
    'Paste the complete callback URL or the authorization code returned after sign-in. The code is paired with this sign-in attempt automatically.',
    'Waiting for authorization…',
  ];

  for (const [lang, cat] of Object.entries(catalogs)) {
    for (const key of oauthDialogKeys) {
      assert.ok(key in cat, `OAuth dialog key "${key}" missing in ${lang}`);
      const val = (cat as Record<string, string>)[key];
      assert.ok(typeof val === 'string' && val.length > 0, `OAuth dialog key "${key}" in ${lang} is empty`);
    }
  }
});

