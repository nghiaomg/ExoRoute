<script lang="ts">
  import Button from "../../components/Button.svelte";
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import Input from '../../components/Input.svelte';
  import { untrack } from 'svelte';
  import { api } from '../../lib/api';
  import type { Translate } from '../../lib/format';
  import type { ProviderPreset, ProviderCreateResult, UpstreamProtocol } from '../../lib/types';

  interface Props {
    open?: boolean;
    preset: ProviderPreset | null;
    tr: Translate;
    onClose?: () => void;
    onCreated?: (providerId: string) => void;
  }

  let {
    open = $bindable(false),
    preset,
    tr,
    onClose = () => {},
    onCreated = () => {},
  }: Props = $props();

  // Name and model prefix are preset metadata, not user input: they are shown
  // read-only and sent verbatim when the provider is created.
  let baseUrl = $state('');
  let apiKey = $state('');
  let busy = $state(false);
  let error = $state('');

  const requiresApiKey = $derived(
    !!preset && preset.capabilities.api_keys && preset.default_auth_type !== 'none',
  );
  const canSubmit = $derived(!!preset && !!baseUrl.trim() && (!requiresApiKey || !!apiKey.trim()) && !busy);

  $effect(() => {
    if (open && preset) {
      untrack(() => {
        baseUrl = '';
        apiKey = '';
        error = '';
        busy = false;
      });
    }
  });

  async function save(): Promise<void> {
    if (!preset || busy || !canSubmit) return;
    busy = true;
    error = '';
    try {
      const result: ProviderCreateResult = await api.createProvider({
        adapter_id: preset.adapter_id,
        name: preset.name,
        base_url: baseUrl.trim(),
        logo_url: preset.default_logo_url ?? null,
        model_prefix: preset.default_model_prefix || undefined,
        enabled: true,
        auth_type: preset.default_auth_type,
        api_keys: apiKey.trim() ? [apiKey.trim()] : [],
        preferred_protocol: preset.default_preferred_protocol,
        supported_protocols: preset.default_supported_protocols as UpstreamProtocol[],
      });
      open = false;
      onCreated(result.id);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : tr('Could not save provider.');
    } finally {
      busy = false;
    }
  }
</script>

<ArkDialog
  bind:open
  wide={true}
  preventClose={busy}
  closeLabel={tr('Close dialog')}
  title={preset?.name ?? tr('Provider setup')}
  kicker={tr('Connect an upstream AI provider.')}
  onClose={onClose}
>
  {#if preset}
    <form class="modal-form" onsubmit={(event) => { event.preventDefault(); void save(); }}>
      <div class="modal-fields">
        <div class="field prov-field-wide">
          <span>{tr('Provider name')}</span>
          <p class="form-help" style="margin:0">{preset.name}</p>
        </div>
        <ArkField
          class="prov-field-wide"
          label={tr('Base URL')}
          type="url"
          bind:value={baseUrl}
          required
          disabled={busy}
          placeholder="https://api.example.com/v1"
        />
        {#if preset.capabilities.api_keys}
          <label class="field prov-field-wide">
            {tr(preset.default_auth_type === 'vertex_service_account' ? 'Service account key' : 'API key')}
            <Input bind:value={apiKey} type="password" autocomplete="off" required={requiresApiKey} disabled={busy} />
          </label>
        {/if}
        <div class="field prov-field-wide">
          <span>{tr('Model prefix (optional)')}</span>
          <p class="form-help" style="margin:0"><code>{preset.default_model_prefix ?? preset.id}</code></p>
        </div>
      </div>
      {#if error}<p class="form-error" role="alert">{error}</p>{/if}
      <div class="modal-actions">
        <Button class="page-action" type="button" data-close-modal disabled={busy}>{tr('Cancel')}</Button>
        <Button class="page-action primary" type="submit" disabled={!canSubmit}>{busy ? tr('Creating…') : tr('Save provider')}</Button>
      </div>
    </form>
  {/if}
</ArkDialog>
