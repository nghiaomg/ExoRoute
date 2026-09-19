<script lang="ts">
  import { Check, LoaderCircle, Plus, X } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
  import { localizedError } from '../../lib/errors';
  import type { Provider, ProviderKeyStrategy as KeyStrategyType, ProviderThinkingMode } from '../../lib/types';
  import ProviderKeyStrategy from './ProviderKeyStrategy.svelte';
  import ProviderThinkingSettings from './ProviderThinkingSettings.svelte';

  export let open = false;
  export let provider: Provider | null = null;
  export let tr: Translate;
  export let onClose: () => void = () => {};
  export let onProviderUpdated: (provider: Provider) => void = () => {};

  type HeaderDraft = { name: string; value: string; existing: boolean };

  let thinkingModeDraft: ProviderThinkingMode = 'preserve';
  let thinkingOverrideDraft = '';
  let keyStrategyDraft: KeyStrategyType = 'priority';
  let modelPrefixDraft = '';
  let logoDraft = '';
  let customHeaderDrafts: HeaderDraft[] = [];

  let saving = false;
  let dialogNotice = '';
  let dialogNoticeTone: 'success' | 'error' = 'success';
  let providerId = '';

  $: if (provider && provider.id !== providerId) {
    providerId = provider.id;
    thinkingModeDraft = provider.thinking_mode ?? 'preserve';
    thinkingOverrideDraft = provider.thinking_override ?? '';
    keyStrategyDraft = provider.key_strategy ?? 'priority';
    modelPrefixDraft = provider.model_prefix ?? provider.id ?? '';
    logoDraft = provider.logo_url ?? '';
    customHeaderDrafts = (provider.custom_headers ?? []).map((name) => ({
      name,
      value: '',
      existing: true,
    }));
    dialogNotice = '';
    dialogNoticeTone = 'success';
  }

  function addCustomHeader(): void {
    customHeaderDrafts = [...customHeaderDrafts, { name: '', value: '', existing: false }];
  }

  function removeCustomHeader(index: number): void {
    customHeaderDrafts = customHeaderDrafts.filter((_, headerIndex) => headerIndex !== index);
  }

  function updateCustomHeader(index: number, field: 'name' | 'value', value: string): void {
    customHeaderDrafts = customHeaderDrafts.map((header, headerIndex) =>
      headerIndex === index ? { ...header, [field]: value } : header,
    );
  }

  async function saveAll(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider || saving) return;

    const currentProvider = provider;
    dialogNotice = '';

    const replacement = thinkingModeDraft === 'override' ? thinkingOverrideDraft.trim() : null;
    const replacementBytes = replacement === null ? 0 : new TextEncoder().encode(replacement).length;
    if (thinkingModeDraft === 'override' && (!replacement || replacementBytes > 4096 || replacement.includes('\0'))) {
      dialogNotice = tr('Replacement must be non-empty and no longer than 4,096 UTF-8 bytes.');
      dialogNoticeTone = 'error';
      return;
    }

    const modelPrefix = modelPrefixDraft.trim().toLowerCase() || currentProvider.id.toLowerCase();
    const logoUrl = logoDraft.trim() || null;
    const customHeaders = customHeaderDrafts
      .map((header) => ({
        name: header.name.trim(),
        value: header.existing && !header.value ? null : header.value,
      }))
      .filter((header) => header.name);

    const initialThinkingMode = currentProvider.thinking_mode ?? 'preserve';
    const initialThinkingOverride = currentProvider.thinking_override ?? null;
    const thinkingChanged =
      thinkingModeDraft !== initialThinkingMode ||
      replacement !== initialThinkingOverride;

    const initialKeyStrategy = currentProvider.key_strategy ?? 'priority';
    const keyStrategyChanged = keyStrategyDraft !== initialKeyStrategy;

    const initialPrefix = (currentProvider.model_prefix ?? '').trim().toLowerCase() || currentProvider.id.toLowerCase();
    const initialLogo = (currentProvider.logo_url ?? '').trim() || null;
    const initialCustomHeaderNames = currentProvider.custom_headers ?? [];
    const headersChanged =
      customHeaders.length !== initialCustomHeaderNames.length ||
      customHeaders.some((h, i) => h.name !== initialCustomHeaderNames[i] || h.value !== null);
    const mainChanged =
      modelPrefix !== initialPrefix ||
      logoUrl !== initialLogo ||
      headersChanged;

    if (!mainChanged && !thinkingChanged && !keyStrategyChanged) {
      dialogNotice = tr('Provider configurations saved.');
      dialogNoticeTone = 'success';
      return;
    }

    saving = true;
    try {
      // 1. Update main provider attributes if changed
      if (mainChanged) {
        await api.updateProvider(currentProvider.id, {
          id: currentProvider.id,
          adapter_id: currentProvider.adapter_id,
          name: currentProvider.name,
          base_url: currentProvider.base_url,
          logo_url: logoUrl,
          model_prefix: modelPrefix,
          enabled: currentProvider.enabled,
          auth_type: currentProvider.auth_type,
          auth_header: currentProvider.auth_header,
          custom_headers: customHeaders,
          local_rpm_target: currentProvider.local_rpm_target,
          preferred_protocol: currentProvider.preferred_protocol,
          supported_protocols: currentProvider.supported_protocols,
        });
      }

      // 2. Update thinking settings if changed
      if (thinkingChanged) {
        await api.updateProviderThinkingSettings(currentProvider.id, {
          mode: thinkingModeDraft,
          override_text: replacement,
        });
      }

      // 3. Update key rotation strategy if changed
      if (keyStrategyChanged) {
        await api.updateProviderKeyStrategy(currentProvider.id, {
          strategy: keyStrategyDraft,
        });
      }

      const headerNames = customHeaders.map((header) => header.name);
      customHeaderDrafts = headerNames.map((name) => ({ name, value: '', existing: true }));
      logoDraft = logoUrl ?? '';
      modelPrefixDraft = modelPrefix;

      const updatedProvider: Provider = {
        ...currentProvider,
        logo_url: logoUrl,
        model_prefix: modelPrefix,
        custom_headers: headerNames,
        thinking_mode: thinkingModeDraft,
        thinking_override: replacement,
        key_strategy: keyStrategyDraft,
      };

      onProviderUpdated(updatedProvider);
      dialogNotice = tr('Provider configurations saved.');
      dialogNoticeTone = 'success';
    } catch (error) {
      dialogNotice = localizedError(error, 'Could not update provider configurations.', tr);
      dialogNoticeTone = 'error';
    } finally {
      saving = false;
    }
  }
</script>

<ArkDialog
  bind:open
  wide={true}
  closeLabel={tr('Close dialog')}
  title={tr('Provider Configurations')}
  kicker={provider?.name ?? tr('Provider details')}
  preventClose={saving}
  onClose={onClose}
>
  {#if provider}
    <form class="modal-form" onsubmit={saveAll}>
      <ProviderThinkingSettings
        bind:mode={thinkingModeDraft}
        bind:overrideText={thinkingOverrideDraft}
        disabled={saving}
        {tr}
      />

      <ProviderKeyStrategy
        bind:strategy={keyStrategyDraft}
        disabled={saving}
        {tr}
      />

      <div class="detail-form-section">
        <div>
          <ArkField
            label={tr('Model prefix')}
            bind:value={modelPrefixDraft}
            maxlength={64}
            disabled={saving}
            placeholder={tr('Defaults to the provider ID')}
          />
        </div>
      </div>

      <div class="detail-form-section">
        <div>
          <ArkField
            label={tr('Logo image URL')}
            bind:value={logoDraft}
            type="url"
            disabled={saving}
            placeholder="https://example.com/logo.png"
          />
        </div>
      </div>

      <div class="detail-form-section">
        <div class="provider-key-drafts-header">
          <div class="form-section-label">
            {tr('Custom headers')} <span>{tr('Sent to the provider with every request')}</span>
          </div>
          <button
            type="button"
            class="secondary-button compact"
            disabled={saving}
            onclick={addCustomHeader}
          >
            <Plus size={13} />
            {tr('Add header')}
          </button>
        </div>
        {#if customHeaderDrafts.length}
          <div class="provider-custom-headers-scroll">
            {#each customHeaderDrafts as header, index (index)}
              <div class="provider-custom-header-row">
                <input
                  value={header.name}
                  aria-label={tr('Header name')}
                  placeholder="x-test"
                  maxlength={128}
                  readonly={header.existing}
                  disabled={saving}
                  oninput={(event) => updateCustomHeader(index, 'name', event.currentTarget.value)}
                />
                <input
                  value={header.value}
                  aria-label={tr('Header value')}
                  placeholder={header.existing ? tr('Leave blank to keep the current value') : 'bla bla'}
                  maxlength={4096}
                  disabled={saving}
                  oninput={(event) => updateCustomHeader(index, 'value', event.currentTarget.value)}
                />
                <button
                  type="button"
                  class="row-icon danger-hover"
                  disabled={saving}
                  aria-label={tr('Remove header')}
                  title={tr('Remove header')}
                  onclick={() => removeCustomHeader(index)}
                >
                  <X size={15} />
                </button>
              </div>
            {/each}
          </div>
        {/if}
        <p class="form-help">{tr('Existing values stay encrypted; enter a new value to replace one.')}</p>
      </div>

      {#if dialogNotice}
        <div
          class="detail-notice-banner"
          class:error={dialogNoticeTone === 'error'}
          role={dialogNoticeTone === 'error' ? 'alert' : 'status'}
        >
          {dialogNotice}
        </div>
      {/if}

      <div class="modal-actions" style="margin-top: 16px;">
        <button
          type="button"
          class="secondary-button"
          disabled={saving}
          onclick={onClose}
        >
          {tr('Cancel')}
        </button>
        <button
          type="submit"
          class="primary-button"
          disabled={saving}
        >
          {#if saving}
            <LoaderCircle size={14} class="spin" />
          {:else}
            <Check size={14} />
          {/if}
          {tr('Save changes')}
        </button>
      </div>
    </form>
  {/if}
</ArkDialog>
