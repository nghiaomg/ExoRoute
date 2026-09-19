<script lang="ts">
  import { Check, LoaderCircle, Plus, X } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Provider } from '../../lib/types';
  import ProviderKeyStrategy from './ProviderKeyStrategy.svelte';
  import ProviderThinkingSettings from './ProviderThinkingSettings.svelte';

  export let open = false;
  export let provider: Provider | null = null;
  export let tr: Translate;
  export let onClose: () => void = () => {};
  export let onProviderUpdated: (provider: Provider) => void = () => {};

  let logoDraft = '';
  let logoNotice = '';
  let logoNoticeTone: 'success' | 'error' = 'success';
  let savingLogo = false;

  let modelPrefixDraft = '';
  let modelPrefixNotice = '';
  let modelPrefixNoticeTone: 'success' | 'error' = 'success';
  let savingModelPrefix = false;
  type HeaderDraft = { name: string; value: string; existing: boolean };
  let customHeaderDrafts: HeaderDraft[] = [];
  let customHeadersNotice = '';
  let customHeadersNoticeTone: 'success' | 'error' = 'success';
  let savingCustomHeaders = false;

  let providerId = '';

  $: if (provider && provider.id !== providerId) {
    providerId = provider.id;
    logoDraft = provider.logo_url ?? '';
    logoNotice = '';
    logoNoticeTone = 'success';
    modelPrefixDraft = provider.model_prefix ?? provider.id ?? '';
    modelPrefixNotice = '';
    modelPrefixNoticeTone = 'success';
    customHeaderDrafts = (provider.custom_headers ?? []).map((name) => ({
      name,
      value: '',
      existing: true,
    }));
    customHeadersNotice = '';
    customHeadersNoticeTone = 'success';
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

  async function saveCustomHeaders(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider || savingLogo || savingModelPrefix || savingCustomHeaders) return;
    const currentProvider = provider;
    customHeadersNotice = '';
    savingCustomHeaders = true;
    try {
      const customHeaders = customHeaderDrafts
        .map((header) => ({
          name: header.name.trim(),
          value: header.existing && !header.value ? null : header.value,
        }))
        .filter((header) => header.name);
      await api.updateProvider(currentProvider.id, {
        id: currentProvider.id,
        adapter_id: currentProvider.adapter_id,
        name: currentProvider.name,
        base_url: currentProvider.base_url,
        logo_url: currentProvider.logo_url,
        model_prefix: currentProvider.model_prefix,
        enabled: currentProvider.enabled,
        auth_type: currentProvider.auth_type,
        auth_header: currentProvider.auth_header,
        custom_headers: customHeaders,
        local_rpm_target: currentProvider.local_rpm_target,
        preferred_protocol: currentProvider.preferred_protocol,
        supported_protocols: currentProvider.supported_protocols,
      });
      const names = customHeaders.map((header) => header.name);
      customHeaderDrafts = names.map((name) => ({ name, value: '', existing: true }));
      customHeadersNotice = tr('Provider custom headers saved.');
      customHeadersNoticeTone = 'success';
      onProviderUpdated({ ...currentProvider, custom_headers: names });
    } catch (error) {
      customHeadersNotice = localizedError(error, 'Could not update provider custom headers.', tr);
      customHeadersNoticeTone = 'error';
    } finally {
      savingCustomHeaders = false;
    }
  }

  async function saveModelPrefix(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider || savingLogo || savingModelPrefix) return;
    const currentProvider = provider;
    const modelPrefix = modelPrefixDraft.trim().toLowerCase() || currentProvider.id.toLowerCase();
    modelPrefixNotice = '';
    savingModelPrefix = true;
    try {
      await api.updateProvider(currentProvider.id, {
        id: currentProvider.id,
        adapter_id: currentProvider.adapter_id,
        name: currentProvider.name,
        base_url: currentProvider.base_url,
        logo_url: currentProvider.logo_url,
        model_prefix: modelPrefix,
        enabled: currentProvider.enabled,
        auth_type: currentProvider.auth_type,
        auth_header: currentProvider.auth_header,
        local_rpm_target: currentProvider.local_rpm_target,
        preferred_protocol: currentProvider.preferred_protocol,
        supported_protocols: currentProvider.supported_protocols,
      });
      const updated = { ...currentProvider, model_prefix: modelPrefix };
      modelPrefixDraft = modelPrefix;
      modelPrefixNotice = tr('Provider model prefix saved.');
      modelPrefixNoticeTone = 'success';
      onProviderUpdated(updated);
    } catch (error) {
      modelPrefixNotice = localizedError(error, 'Could not update provider model prefix.', tr);
      modelPrefixNoticeTone = 'error';
    } finally {
      savingModelPrefix = false;
    }
  }

  async function saveLogo(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider || savingLogo || savingModelPrefix) return;
    const currentProvider = provider;
    const logoUrl = logoDraft.trim() || null;
    logoNotice = '';
    savingLogo = true;
    try {
      await api.updateProvider(currentProvider.id, {
        id: currentProvider.id,
        adapter_id: currentProvider.adapter_id,
        name: currentProvider.name,
        base_url: currentProvider.base_url,
        logo_url: logoUrl,
        model_prefix: currentProvider.model_prefix,
        enabled: currentProvider.enabled,
        auth_type: currentProvider.auth_type,
        auth_header: currentProvider.auth_header,
        local_rpm_target: currentProvider.local_rpm_target,
        preferred_protocol: currentProvider.preferred_protocol,
        supported_protocols: currentProvider.supported_protocols,
      });
      const updated = { ...currentProvider, logo_url: logoUrl };
      logoDraft = logoUrl ?? '';
      logoNotice = tr('Provider logo saved.');
      logoNoticeTone = 'success';
      onProviderUpdated(updated);
    } catch (error) {
      logoNotice = localizedError(error, 'Could not update provider logo.', tr);
      logoNoticeTone = 'error';
    } finally {
      savingLogo = false;
    }
  }

</script>

<ArkDialog
  bind:open
  wide={true}
  closeLabel={tr('Close dialog')}
  title={tr('Provider Configurations')}
  kicker={provider?.name ?? tr('Provider details')}
  onClose={onClose}
>
  {#if provider}
    <div class="modal-form">
      <ProviderThinkingSettings provider={provider} {tr} onUpdated={onProviderUpdated} />

      <ProviderKeyStrategy provider={provider} {tr} onUpdated={onProviderUpdated} />

      <form class="detail-form-section" onsubmit={saveModelPrefix}>
        <div>
          <ArkField
            label={tr('Model prefix')}
            bind:value={modelPrefixDraft}
            maxlength={64}
            placeholder={tr('Defaults to the provider ID')}
          />
        </div>
        {#if modelPrefixNotice}
          <div
            class="detail-notice-banner"
            class:error={modelPrefixNoticeTone === 'error'}
            role={modelPrefixNoticeTone === 'error' ? 'alert' : 'status'}
          >
            {modelPrefixNotice}
          </div>
        {/if}
        <div style="display: flex; justify-content: flex-end;">
          <button
            type="submit"
            class="primary-button compact"
            disabled={savingLogo || savingModelPrefix}
          >
            {#if savingModelPrefix}
              <LoaderCircle size={13} class="spin" />
            {:else}
              <Check size={13} />
            {/if}
            {tr('Save model prefix')}
          </button>
        </div>
      </form>

      <form class="detail-form-section" onsubmit={saveLogo}>
        <div>
          <ArkField
            label={tr('Logo image URL')}
            bind:value={logoDraft}
            type="url"
            placeholder="https://example.com/logo.png"
          />
        </div>
        {#if logoNotice}
          <div
            class="detail-notice-banner"
            class:error={logoNoticeTone === 'error'}
            role={logoNoticeTone === 'error' ? 'alert' : 'status'}
          >
            {logoNotice}
          </div>
        {/if}
        <div style="display: flex; justify-content: flex-end;">
          <button
            type="submit"
            class="primary-button compact"
            disabled={savingLogo || savingModelPrefix}
          >
            {#if savingLogo}
              <LoaderCircle size={13} class="spin" />
            {:else}
              <Check size={13} />
            {/if}
            {tr('Save logo')}
          </button>
        </div>
      </form>

      <form class="detail-form-section" onsubmit={saveCustomHeaders}>
        <div class="provider-key-drafts-header">
          <div class="form-section-label">{tr('Custom headers')} <span>{tr('Sent to the provider with every request')}</span></div>
          <button type="button" class="secondary-button compact" onclick={addCustomHeader}><Plus size={13} />{tr('Add header')}</button>
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
                  oninput={(event) => updateCustomHeader(index, 'name', event.currentTarget.value)}
                />
                <input
                  value={header.value}
                  aria-label={tr('Header value')}
                  placeholder={header.existing ? tr('Leave blank to keep the current value') : 'bla bla'}
                  maxlength={4096}
                  oninput={(event) => updateCustomHeader(index, 'value', event.currentTarget.value)}
                />
                <button type="button" class="row-icon danger-hover" aria-label={tr('Remove header')} title={tr('Remove header')} onclick={() => removeCustomHeader(index)}><X size={15} /></button>
              </div>
            {/each}
          </div>
        {/if}
        <p class="form-help">{tr('Existing values stay encrypted; enter a new value to replace one.')}</p>
        {#if customHeadersNotice}
          <div class="detail-notice-banner" class:error={customHeadersNoticeTone === 'error'} role={customHeadersNoticeTone === 'error' ? 'alert' : 'status'}>{customHeadersNotice}</div>
        {/if}
        <div style="display: flex; justify-content: flex-end;">
          <button type="submit" class="primary-button compact" disabled={savingLogo || savingModelPrefix || savingCustomHeaders}>
            {#if savingCustomHeaders}<LoaderCircle size={13} class="spin" />{:else}<Check size={13} />{/if}
            {tr('Save custom headers')}
          </button>
        </div>
      </form>
    </div>
  {/if}
</ArkDialog>
