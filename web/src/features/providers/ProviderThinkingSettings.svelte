<script lang="ts">
  import ArkSelect from '../../components/ArkSelect.svelte';
  import { type Translate } from '../../lib/format';
  import type { ProviderThinkingMode } from '../../lib/types';

  export let mode: ProviderThinkingMode = 'preserve';
  export let overrideText = '';
  export let disabled = false;
  export let tr: Translate;

  const MAX_OVERRIDE_BYTES = 4096;

  $: thinkingOptions = [
    { label: tr('Preserve original'), value: 'preserve' },
    { label: tr('Override with custom text'), value: 'override' },
    { label: tr('Remove thinking blocks'), value: 'remove' },
  ];
</script>

<div class="detail-form-section provider-thinking-settings">
  <div>
    <ArkSelect
      label={tr('Thinking block handling')}
      items={thinkingOptions}
      value={mode}
      disabled={disabled}
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
        disabled={disabled}
        placeholder={tr('Replacement text')}
      ></textarea>
    </div>
  {/if}
</div>

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
  :global(:root[data-theme='dark']) .provider-thinking-label { color: #d0d2df; }
  :global(:root[data-theme='dark']) .provider-thinking-textarea { border-color: #404252; background: #222330; color: #e3e4ee; }
</style>
