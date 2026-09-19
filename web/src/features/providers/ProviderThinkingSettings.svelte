<script lang="ts">
  import { Check, LoaderCircle } from '@lucide/svelte';
  import ArkSelect from '../../components/ArkSelect.svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Provider, ProviderThinkingMode } from '../../lib/types';

  export let provider: Provider;
  export let tr: Translate;
  export let onUpdated: (provider: Provider) => void;

  const MAX_OVERRIDE_BYTES = 4096;
  let providerId = '';
  let mode: ProviderThinkingMode = 'preserve';
  let overrideText = '';
  let saving = false;
  let notice = '';
  let noticeTone: 'success' | 'error' = 'success';

  $: if (provider && provider.id !== providerId) {
    providerId = provider.id;
    mode = provider.thinking_mode ?? 'preserve';
    overrideText = provider.thinking_override ?? '';
    notice = '';
    noticeTone = 'success';
  }

  $: thinkingOptions = [
    { label: tr('Preserve original'), value: 'preserve' },
    { label: tr('Override with custom text'), value: 'override' },
    { label: tr('Remove thinking blocks'), value: 'remove' },
  ];

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider || saving) return;

    const targetProvider = provider;
    const savedMode = mode;
    const replacement = mode === 'override' ? overrideText : null;
    const replacementBytes = replacement === null ? 0 : new TextEncoder().encode(replacement).length;
    if (mode === 'override' && (!replacement?.trim() || replacementBytes > MAX_OVERRIDE_BYTES || replacement.includes('\0'))) {
      notice = tr('Replacement must be non-empty and no longer than 4,096 UTF-8 bytes.');
      noticeTone = 'error';
      return;
    }

    saving = true;
    notice = '';
    try {
      await api.updateProviderThinkingSettings(targetProvider.id, {
        mode: savedMode,
        override_text: replacement,
      });
      notice = tr('Thinking settings saved.');
      noticeTone = 'success';
      onUpdated({
        ...targetProvider,
        thinking_mode: savedMode,
        thinking_override: replacement,
      });
    } catch (error) {
      notice = localizedError(error, 'Could not update provider thinking settings.', tr);
      noticeTone = 'error';
    } finally {
      saving = false;
    }
  }
</script>

<form class="detail-form-section provider-thinking-settings" onsubmit={save}>
  <div>
    <ArkSelect
      label={tr('Thinking block handling')}
      items={thinkingOptions}
      value={mode}
      disabled={saving}
      onValueChange={(val) => {
        mode = (val as ProviderThinkingMode) || 'preserve';
      }}
    />
  </div>

  {#if mode === 'override'}
    <div>
      <label for="provider-thinking-override" class="provider-thinking-label">{tr('Replacement text')}</label>
      <textarea
        id="provider-thinking-override"
        class="provider-thinking-textarea"
        bind:value={overrideText}
        maxlength={MAX_OVERRIDE_BYTES}
        rows="2"
        disabled={saving}
        placeholder={tr('Replacement text')}
      ></textarea>
    </div>
  {/if}

  {#if notice}
    <div class="detail-notice-banner" class:error={noticeTone === 'error'} role={noticeTone === 'error' ? 'alert' : 'status'}>
      {notice}
    </div>
  {/if}

  <div class="provider-thinking-actions">
    <button type="submit" class="primary-button compact" disabled={saving}>
      {#if saving}<LoaderCircle size={13} class="spin" />{:else}<Check size={13} />{/if}
      {tr('Save thinking settings')}
    </button>
  </div>
</form>

<style>
  .provider-thinking-settings { gap: 10px; }
  .provider-thinking-label { display: block; margin-bottom: 6px; color: #4e5267; font-size: var(--text-xs); font-weight: var(--font-semibold); }
  .provider-thinking-textarea {
    box-sizing: border-box;
    width: 100%;
    border: 1px solid #e5e6ed;
    border-radius: var(--radius-sm);
    background: #fff;
    color: #44485c;
    font: inherit;
    font-size: var(--text-sm);
    min-height: 52px;
    padding: 6px 10px;
    resize: vertical;
  }
  .provider-thinking-actions { display: flex; justify-content: flex-end; }
  :global(:root[data-theme='dark']) .provider-thinking-label { color: #d0d2df; }
  :global(:root[data-theme='dark']) .provider-thinking-textarea { border-color: #404252; background: #222330; color: #e3e4ee; }
</style>
