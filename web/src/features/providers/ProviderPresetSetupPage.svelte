<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { untrack } from 'svelte';
  import { api } from '../../lib/api';
  import type { Translate } from '../../lib/format';
  import type { ProviderPreset, ProviderCreateResult, UpstreamProtocol } from '../../lib/types';

  interface Props { preset: ProviderPreset; tr: Translate; onBack: () => void; onCreated: (providerId: string) => void; }
  let { preset, tr, onBack, onCreated }: Props = $props();
  let name = $state(untrack(() => preset.name));
  let baseUrl = $state('');
  let apiKey = $state('');
  let modelPrefix = $state(untrack(() => preset.default_model_prefix ?? ''));
  const requiresApiKey = $derived(preset.capabilities.api_keys && preset.default_auth_type !== 'none');
  let busy = $state(false);
  let error = $state('');

  async function save(): Promise<void> {
    const providerName = name.trim();
    const endpoint = baseUrl.trim();
    const key = apiKey.trim();
    if (busy || !providerName || !endpoint || (requiresApiKey && !key)) return;
    busy = true;
    error = '';
    try {
      const result: ProviderCreateResult = await api.createProvider({
        adapter_id: preset.adapter_id,
        name: providerName,
        base_url: endpoint,
        logo_url: preset.default_logo_url ?? null,
        model_prefix: modelPrefix.trim() || undefined,
        enabled: true,
        auth_type: preset.default_auth_type,
        api_keys: key ? [key] : [],
        preferred_protocol: preset.default_preferred_protocol,
        supported_protocols: preset.default_supported_protocols as UpstreamProtocol[],
      });
      onCreated(result.id);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : tr('Could not save provider.');
    } finally {
      busy = false;
    }
  }
</script>

<div class="provider-detail-page provider-preset-setup">
  <div class="detail-page-nav">
    <Button class="back-link-btn" type="button" onclick={onBack}>
      <span aria-hidden="true">←</span>
      <span>{tr('Back to providers')}</span>
    </Button>
  </div>
  <header class="detail-hero-banner">
    <div class="detail-hero-info"><h1 class="detail-title">{preset.name}</h1><p>{tr('Connect an upstream AI provider.')}</p></div>
  </header>
  <section class="detail-card">
    <form class="modal-card" onsubmit={(event) => { event.preventDefault(); void save(); }}>
      <div class="modal-fields">
        <label class="field prov-field-wide">{tr('Provider name')}<input bind:value={name} maxlength="60" required /></label>
        <label class="field prov-field-wide">{tr('Base URL')}<input bind:value={baseUrl} type="url" required placeholder="https://api.example.com/v1" /></label>
        {#if preset.capabilities.api_keys}
          <label class="field prov-field-wide">{tr(preset.default_auth_type === 'vertex_service_account' ? 'Service account key' : 'API key')}<input bind:value={apiKey} type="password" autocomplete="off" required={requiresApiKey} /></label>
        {/if}
        <label class="field prov-field-wide">{tr('Model prefix (optional)')}<input bind:value={modelPrefix} maxlength="120" /></label>
      </div>
      {#if error}<p class="form-error" role="alert">{error}</p>{/if}
      <div class="modal-actions"><Button class="page-action" type="button" onclick={onBack}>{tr('Cancel')}</Button><Button class="page-action primary" type="submit" disabled={busy || !name.trim() || !baseUrl.trim()}>{busy ? tr('Creating…') : tr('Save provider')}</Button></div>
    </form>
  </section>
</div>
