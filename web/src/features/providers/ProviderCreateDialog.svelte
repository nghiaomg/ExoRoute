<script lang="ts">
  import { LoaderCircle, Plus, X } from '@lucide/svelte';
  import Dialog from '../../components/ui/Dialog.svelte';
  import FormField from '../../components/ui/FormField.svelte';
  import PasswordInput from '../../components/ui/PasswordInput.svelte';
  import Button from '../../components/ui/Button.svelte';
  import Input from '../../components/ui/Input.svelte';
  import SelectOptions from '../../components/ui/SelectOptions.svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import { labelAuthType, labelProtocol } from '../../lib/labels';
  import { type Translate } from '../../lib/format';
  import type { Protocol, ProviderCreateResult, ProviderPreset } from '../../lib/types';
  interface Props { open: boolean; presets: ProviderPreset[]; tr: Translate; onSaved: (id: string, adapterId: string) => void; onClose?: () => void }
  let { open = $bindable(false), presets = [], tr, onSaved, onClose = () => {} }: Props = $props();
  const DEFAULT_PROTOCOLS: Protocol[] = ['chat_completions', 'responses', 'messages'];
  let name = $state(''); let baseUrl = $state(''); let logoUrl = $state(''); let modelPrefix = $state('');
  let keyDrafts = $state<string[]>(['']); let headerDrafts = $state<Array<{ name: string; value: string }>>([]);
  let authType = $state('bearer'); let authHeader = $state('x-api-key'); let protocol = $state<Protocol>('chat_completions');
  let result = $state<ProviderCreateResult | null>(null); let errorMessage = $state(''); let saving = $state(false);
  let fieldErrors = $state<{ name?: string; baseUrl?: string }>({});
  let wasOpen = false;
  const REQUIRED_MARK = ' *';

  /** Inline validation for required fields; runs before any API call. */
  function validateFields(): boolean {
    const errors: { name?: string; baseUrl?: string } = {};
    if (!name.trim()) errors.name = tr('provider name is required');
    if (!baseUrl.trim()) errors.baseUrl = tr('Base URL is required.');
    else {
      try {
        const parsed = new URL(baseUrl.trim());
        if (!/^https?:$/.test(parsed.protocol)) errors.baseUrl = tr('Base URL must be a valid URL.');
      } catch {
        errors.baseUrl = tr('Base URL must be a valid URL.');
      }
    }
    fieldErrors = errors;
    return Object.keys(errors).length === 0;
  }
  const customPreset = $derived(presets.find((p) => p.id === 'custom' || p.adapter_id === 'generic') ?? null);
  const protocols = $derived.by(() => {
    const values = (customPreset?.default_supported_protocols?.length ? customPreset.default_supported_protocols : DEFAULT_PROTOCOLS)
      .map((value) => String(value))
      .filter((value): value is Protocol => value === 'chat_completions' || value === 'responses' || value === 'messages');
    return values.length ? values : DEFAULT_PROTOCOLS;
  });
  const authTypes = $derived((customPreset?.supported_auth_types ?? ['bearer', 'header']).filter((value) => value !== 'none'));
  $effect(() => {
    if (open && !wasOpen) { wasOpen = true; reset(); } else if (!open) wasOpen = false;
  });
  function reset(): void {
    result = null; errorMessage = ''; fieldErrors = {}; name = ''; baseUrl = ''; logoUrl = ''; modelPrefix = '';
    keyDrafts = ['']; headerDrafts = []; authType = customPreset?.supported_auth_types?.includes('bearer') ? 'bearer' : (customPreset?.default_auth_type ?? 'bearer');
    authHeader = 'x-api-key'; protocol = protocols[0] ?? 'chat_completions';
  }
  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault(); if (!validateFields()) return;
    if (!protocols.length || saving) { errorMessage = tr('Custom provider settings are unavailable. Reload the page and try again.'); return; }
    errorMessage = ''; saving = true;
    try {
      const keys = keyDrafts.map((key) => key.trim()).filter(Boolean);
      const supported = protocols.includes(protocol) ? protocols : [protocol, ...protocols];
      result = await api.createProvider({
        adapter_id: customPreset?.adapter_id ?? 'generic',
        name: name.trim(),
        base_url: baseUrl.trim(),
        logo_url: logoUrl.trim() || null,
        model_prefix: modelPrefix.trim() || undefined,
        enabled: true,
        auth_type: keys.length ? authType : 'none',
        auth_header: keys.length && authType === 'header' ? authHeader.trim() : undefined,
        custom_headers: headerDrafts.map((h) => ({ name: h.name.trim(), value: h.value })).filter((h) => h.name),
        api_keys: keys,
        preferred_protocol: protocol,
        supported_protocols: supported,
      });
      onSaved(result.id, customPreset?.adapter_id ?? 'generic');
    } catch (error) { result = null; errorMessage = localizedError(error, 'Could not save this provider.', tr); }
    finally { saving = false; }
  }
</script>

<Dialog bind:open {onClose} wide class="provider-create-dialog" closeLabel={tr('Close dialog')} title={tr(result ? 'Provider key tests' : 'Add a custom provider')} kicker={tr('EXOROUTE CONTROL PLANE')}>
  {#if result}
    <div class="modal-form">
      <p class="form-help" role="status">{tr('Provider saved. One or more API key tests failed; those keys are still saved and can be managed below.')}</p>
      <div class="modal-actions"><Button class="page-action primary" type="button" onclick={() => (open = false)}>{tr('Done')}</Button></div>
    </div>
  {:else}
    <form class="modal-form provider-create-form" novalidate onsubmit={submit}>
      <div class="grid gap-3 sm:grid-cols-2">
        <FormField label={tr('Provider name') + REQUIRED_MARK} bind:value={name} required errorText={fieldErrors.name} onValueChange={() => (fieldErrors = { ...fieldErrors, name: undefined })} placeholder={tr('e.g. OpenAI compatible')} />
        <FormField label={tr('Model prefix')} bind:value={modelPrefix} maxlength={64} placeholder={tr('Defaults to the provider ID')} />
        <FormField label={tr('Base URL') + REQUIRED_MARK} bind:value={baseUrl} type="url" required errorText={fieldErrors.baseUrl} onValueChange={() => (fieldErrors = { ...fieldErrors, baseUrl: undefined })} placeholder="https://api.example.com/v1" />
        <FormField label={tr('Logo image URL')} bind:value={logoUrl} type="url" placeholder="https://example.com/logo.png" />
        <SelectOptions ariaLabel={tr('Preferred protocol')} label={tr('Preferred protocol')} bind:value={protocol} items={protocols.map((p) => ({ label: labelProtocol(p, tr), value: p }))} />
        {#if keyDrafts.some((key) => key.trim())}
          <SelectOptions ariaLabel={tr('Authentication')} label={tr('Authentication')} bind:value={authType} items={authTypes.map((type) => ({ label: labelAuthType(type, tr), value: type }))} />
          {#if authType === 'header'}<FormField label={tr('Header name')} bind:value={authHeader} required placeholder="x-api-key" />{/if}
        {/if}
      </div>
      <div class="grid gap-2">
        <div class="flex items-center justify-between gap-2"><span class="form-section-label provider-section-label">{tr('API keys')} <span>{tr('Each key is tested before it is saved')}</span></span><Button class="secondary-button compact" type="button" onclick={() => (keyDrafts = [...keyDrafts, ''])}><Plus size={13} />{tr('Add another key')}</Button></div>
        {#each keyDrafts as key, index (index)}
          <div class="provider-key-input-row"><PasswordInput class="provider-key-input" bind:value={keyDrafts[index]} autocomplete="new-password" ariaLabel={tr('API key {index}', { index: index + 1 })} placeholder={tr('API key {index}', { index: index + 1 })} visibilityToggleLabel={tr('Toggle password visibility')} />{#if keyDrafts.length > 1}<Button class="row-icon danger-hover" type="button" aria-label={tr('Remove key field')} title={tr('Remove key field')} onclick={() => (keyDrafts = keyDrafts.filter((_, i) => i !== index))}><X size={15} /></Button>{/if}</div>
        {/each}
        <p class="form-help key-test-help">{tr('Keys are tested while saving. A failed test shows a warning, but the provider and key are still saved.')}</p>
      </div>
      <div class="grid gap-2">
        <div class="flex items-center justify-between gap-2"><span class="form-section-label provider-section-label">{tr('Custom headers')}</span><Button class="secondary-button compact" type="button" onclick={() => (headerDrafts = [...headerDrafts, { name: '', value: '' }])}><Plus size={13} />{tr('Add header')}</Button></div>
        {#each headerDrafts as header, index (index)}
          <div class="flex items-center gap-2"><Input value={header.name} aria-label={tr('Header name')} placeholder={tr('Header name')} maxlength={128} oninput={(e) => (headerDrafts[index] = { ...header, name: e.currentTarget.value })} /><Input value={header.value} aria-label={tr('Header value')} placeholder={tr('Header value')} maxlength={4096} oninput={(e) => (headerDrafts[index] = { ...header, value: e.currentTarget.value })} /><Button class="row-icon danger-hover" type="button" aria-label={tr('Remove header')} title={tr('Remove header')} onclick={() => (headerDrafts = headerDrafts.filter((_, i) => i !== index))}><X size={15} /></Button></div>
        {/each}
      </div>
      {#if errorMessage}<div class="form-error" role="alert">{errorMessage}</div>{/if}
      <div class="modal-actions"><Button class="page-action" type="button" onclick={() => (open = false)}>{tr('Cancel')}</Button><Button class="page-action primary" type="submit" disabled={saving}>{#if saving}<LoaderCircle size={15} class="spin" />{:else}<Plus size={15} />{/if}{tr('Add provider')}</Button></div>
    </form>
  {/if}
</Dialog>

<style>
  /* Modal-local corrections: single clean form grid with full-width controls. */
  .provider-create-form .provider-key-input-row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
  }
  .provider-create-form :global(.provider-key-input) {
    flex: 1 1 auto;
    min-width: 0;
  }
  /* The inner eye toggle must not inherit the modal button border rules. */
  .provider-create-form :global(.provider-key-input button) {
    border: 0 !important;
    background: transparent !important;
    box-shadow: none !important;
  }
  .provider-create-form .key-test-help {
    margin: 0;
    text-align: left;
    text-justify: none;
    line-height: 1.5;
  }
  .provider-create-form .provider-section-label {
    color: var(--ink);
  }
</style>
