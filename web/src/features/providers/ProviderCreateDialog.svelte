<script lang="ts">
  import { LoaderCircle, Plus, X } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import ArkPasswordInput from '../../components/ArkPasswordInput.svelte';
  import ArkSelect from '../../components/ArkSelect.svelte';
  import { api } from '../../lib/api';
  import type { Translate } from '../../lib/format';
  import { labelAuthType, labelProtocol } from '../../lib/labels';
import { localizedError } from '../../lib/errors';
  import type { Protocol, ProviderCreateResult, ProviderPreset } from '../../lib/types';

  export let open = false;
  export let presets: ProviderPreset[] = [];
  export let tr: Translate;
  export let onSaved: (id: string, adapterId: string) => void;

  const DEFAULT_CUSTOM_PROTOCOLS: Protocol[] = ['chat_completions', 'responses', 'messages'];

  let providerName = '';
  let providerUrl = '';
  let providerLogoUrl = '';
  let providerModelPrefix = '';
  let providerKeyDrafts = [''];
  type HeaderDraft = { name: string; value: string };
  let providerHeaderDrafts: HeaderDraft[] = [];
  let providerAuthType = 'bearer';
  let providerAuthHeader = 'x-api-key';
  let providerProtocol: Protocol = 'chat_completions';
  let result: ProviderCreateResult | null = null;
  let errorMessage = '';
  let saving = false;
  let wasOpen = false;

  $: activePreset = presets.find((preset) => preset.id === 'custom' || preset.adapter_id === 'generic') ?? null;
  $: selectableProtocols = (activePreset?.default_supported_protocols?.length
    ? activePreset.default_supported_protocols
    : DEFAULT_CUSTOM_PROTOCOLS)
    .map((protocol) => String(protocol))
    .filter((protocol): protocol is Protocol => ['chat_completions', 'responses', 'messages'].includes(protocol as Protocol));
  $: protocolSelectable = true;
  $: supportedAuthTypes = activePreset?.supported_auth_types ?? ['bearer', 'header'];
  $: if (open && !wasOpen) {
    wasOpen = true;
    reset();
  } else if (!open) {
    wasOpen = false;
  }

  function reset(): void {
    result = null;
    errorMessage = '';
    providerName = '';
    providerUrl = '';
    providerLogoUrl = '';
    providerModelPrefix = '';
    providerKeyDrafts = [''];
    providerHeaderDrafts = [];
    providerAuthType = activePreset?.supported_auth_types?.includes('bearer')
      ? 'bearer'
      : (activePreset?.default_auth_type ?? 'bearer');
    providerAuthHeader = 'x-api-key';
    const preferred = activePreset?.default_preferred_protocol;
    providerProtocol = (preferred && selectableProtocols.includes(preferred as Protocol)
      ? preferred
      : (selectableProtocols[0] ?? 'chat_completions')) as Protocol;
  }

  function closeDialog(): void {
    open = false;
  }

  function addKeyDraft(): void {
    providerKeyDrafts = [...providerKeyDrafts, ''];
  }

  function removeKeyDraft(index: number): void {
    providerKeyDrafts = providerKeyDrafts.filter((_, keyIndex) => keyIndex !== index);
    if (!providerKeyDrafts.length) providerKeyDrafts = [''];
  }

  function addHeaderDraft(): void {
    providerHeaderDrafts = [...providerHeaderDrafts, { name: '', value: '' }];
  }

  function removeHeaderDraft(index: number): void {
    providerHeaderDrafts = providerHeaderDrafts.filter((_, headerIndex) => headerIndex !== index);
  }

  function updateHeaderDraft(index: number, field: keyof HeaderDraft, value: string): void {
    providerHeaderDrafts = providerHeaderDrafts.map((header, headerIndex) =>
      headerIndex === index ? { ...header, [field]: value } : header,
    );
  }

  function keyTestMessage(message: string): string {
    const commandCodeStatus = message.match(/^Command Code API key could not be verified \(HTTP (\d+)\)$/);
    if (commandCodeStatus) return tr('Command Code API key could not be verified (HTTP {status}).', { status: commandCodeStatus[1] });
    const commandCodeNetwork = message.match(/^Could not verify Command Code API key: (.+)$/);
    if (commandCodeNetwork) return tr('Could not verify Command Code API key: {reason}', { reason: commandCodeNetwork[1] });
    return message;
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!selectableProtocols.length) {
      errorMessage = tr('Custom provider settings are unavailable. Reload the page and try again.');
      return;
    }
    errorMessage = '';
    saving = true;
    try {
      const keys = providerKeyDrafts.map((key) => key.trim()).filter(Boolean);
      const adapterId = activePreset?.adapter_id ?? 'generic';
      const supported = selectableProtocols.includes(providerProtocol)
        ? selectableProtocols
        : [providerProtocol, ...selectableProtocols];
      result = await api.createProvider({
        adapter_id: adapterId,
        name: providerName.trim(),
        base_url: providerUrl.trim(),
        logo_url: providerLogoUrl.trim() || null,
        model_prefix: providerModelPrefix.trim() || undefined,
        enabled: true,
        auth_type: keys.length ? providerAuthType : 'none',
        auth_header: keys.length && providerAuthType === 'header' ? providerAuthHeader.trim() : undefined,
        custom_headers: providerHeaderDrafts
          .map((header) => ({ name: header.name.trim(), value: header.value }))
          .filter((header) => header.name || header.value),
        api_keys: keys,
        preferred_protocol: providerProtocol,
        supported_protocols: supported,
      });
      onSaved(result.id, adapterId);
    } catch (error) {
      result = null;
      errorMessage = localizedError(error, 'Could not save this provider.', tr);
    } finally {
      saving = false;
    }
  }
</script>

<ArkDialog bind:open wide class="provider-create-dialog" closeLabel={tr('Close dialog')} title={tr(result ? 'Provider key tests' : 'Add a custom provider')} kicker={tr('EXOROUTE CONTROL PLANE')}>
  {#if result}
    <div class="modal-form">
      {#if result.key_tests.some((test) => !test.test_passed)}
        <div class="key-test-warning" role="alert">{tr('Provider saved. One or more API key tests failed; those keys are still saved and can be managed below.')}</div>
      {:else if result.key_tests.length}
        <div class="key-test-success" role="status">{tr('Provider saved and all API key tests passed.')}</div>
      {:else}
        <div class="key-test-success" role="status">{tr('Provider saved without API keys.')}</div>
      {/if}
      {#if result.key_tests.length}
        <div class="key-test-list">
          {#each result.key_tests as test (test.index)}
            <div class="key-test-row" class:failed={!test.test_passed}><span class="key-test-mark">{test.test_passed ? '✓' : '!'}</span><div><strong>{tr('API key {index}', { index: test.index + 1 })} · {tr(test.test_passed ? 'Test passed' : 'Test failed')}</strong>{#if test.status}<small>{tr('HTTP {status}', { status: test.status })}</small>{/if}{#if test.warning || test.message}<small>{keyTestMessage(test.warning || test.message || '')}</small>{/if}</div></div>
          {/each}
        </div>
      {/if}
      <div class="modal-actions"><button type="button" class="primary-button" onclick={closeDialog}>{tr('Done')}</button></div>
    </div>
  {:else}
    <form class="modal-form provider-create-form" onsubmit={submit}>
      <div class="provider-create-grid">
        <ArkField label={tr('Provider name')} bind:value={providerName} required placeholder={tr('e.g. OpenAI compatible')} />
        <ArkField label={tr('Model prefix')} bind:value={providerModelPrefix} maxlength={64} placeholder={tr('Defaults to the provider ID')} />

        <ArkField label={tr('Base URL')} bind:value={providerUrl} type="url" required placeholder="https://api.example.com/v1" />
        <ArkField label={tr('Logo image URL')} bind:value={providerLogoUrl} type="url" placeholder="https://example.com/logo.png" />

        <div class="provider-field-col">
          <ArkSelect
            label={tr('Preferred protocol')}
            bind:value={providerProtocol}
            onValueChange={(val) => { providerProtocol = val as Protocol; }}
            items={selectableProtocols.map((protocol) => ({
              label: labelProtocol(protocol, tr),
              value: protocol,
            }))}
          />
        </div>

        <div class="provider-field-col auth-col">
          {#if providerKeyDrafts.some((key) => key.trim())}
            <div class="auth-flex-row" class:has-header={providerAuthType === 'header'}>
              <div class="auth-type-field">
                <ArkSelect
                  label={tr('Authentication')}
                  bind:value={providerAuthType}
                  onValueChange={(val) => { providerAuthType = val; }}
                  items={supportedAuthTypes
                    .filter((type) => type !== 'none')
                    .map((type) => ({
                      label: labelAuthType(type, tr),
                      value: type,
                    }))}
                />
              </div>
              {#if providerAuthType === 'header'}
                <div class="auth-header-field">
                  <ArkField label={tr('Header name')} bind:value={providerAuthHeader} required placeholder="x-api-key" />
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>

      <div class="provider-key-drafts provider-create-keys-section">
        <div class="provider-key-drafts-header">
          <div class="form-section-label">{tr('API keys')} <span>{tr('Each key is tested before it is saved')}</span></div>
          <button type="button" class="secondary-button compact add-key-field" onclick={addKeyDraft}><Plus size={13} />{tr('Add another key')}</button>
        </div>
        <div class="provider-key-drafts-scroll">
          {#each providerKeyDrafts as key, index (index)}
            <div class="provider-key-input-row">
              <ArkPasswordInput bind:value={providerKeyDrafts[index]} autocomplete="new-password" ariaLabel={tr('API key {index}', { index: index + 1 })} placeholder={tr('API key {index}', { index: index + 1 })} visibilityToggleLabel={tr('Toggle password visibility')} />
              {#if providerKeyDrafts.length > 1}
                <button type="button" class="row-icon danger-hover" aria-label={tr('Remove key field')} title={tr('Remove key field')} onclick={() => removeKeyDraft(index)}><X size={15} /></button>
              {/if}
            </div>
          {/each}
        </div>
        <p class="form-help key-test-help">{tr('Keys are tested while saving. A failed test shows a warning, but the provider and key are still saved.')}</p>
      </div>

      <div class="provider-key-drafts provider-custom-headers-section">
        <div class="provider-key-drafts-header">
          <div class="form-section-label">{tr('Custom headers')} <span>{tr('Sent to the provider with every request')}</span></div>
          <button type="button" class="secondary-button compact add-key-field" onclick={addHeaderDraft}><Plus size={13} />{tr('Add header')}</button>
        </div>
        {#if providerHeaderDrafts.length}
          <div class="provider-custom-headers-scroll">
            {#each providerHeaderDrafts as header, index (index)}
              <div class="provider-custom-header-row">
                <input
                  value={header.name}
                  aria-label={tr('Header name')}
                  placeholder={tr('Header name')}
                  maxlength={128}
                  oninput={(event) => updateHeaderDraft(index, 'name', event.currentTarget.value)}
                />
                <input
                  value={header.value}
                  aria-label={tr('Header value')}
                  placeholder={tr('Header value')}
                  maxlength={4096}
                  oninput={(event) => updateHeaderDraft(index, 'value', event.currentTarget.value)}
                />
                <button type="button" class="row-icon danger-hover" aria-label={tr('Remove header')} title={tr('Remove header')} onclick={() => removeHeaderDraft(index)}><X size={15} /></button>
              </div>
            {/each}
          </div>
        {/if}
        <p class="form-help">{tr('Header names and values are stored securely and are not shown again.')}</p>
      </div>

      {#if errorMessage}<div class="form-error" role="alert">{errorMessage}</div>{/if}
      <div class="modal-actions"><button type="button" class="secondary-button" onclick={closeDialog}>{tr('Cancel')}</button><button class="primary-button" disabled={saving}>{#if saving}<LoaderCircle size={15} class="spin" />{:else}<Plus size={15} />{/if}{tr('Add provider')}</button></div>
    </form>
  {/if}
</ArkDialog>
