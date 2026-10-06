<script lang="ts">
  import { Check, LoaderCircle, Plus } from '@lucide/svelte';
  import ArkClipboard from '../../components/ArkClipboard.svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Provider } from '../../lib/types';
  import ApiKeyScopeFields from './ApiKeyScopeFields.svelte';
  import { parseModelRules } from './scope';

  export let open = false;
  export let tr: Translate;
  export let onClose: () => void;
  export let onCreated: () => void;

  let name = '';
  let createdKey = '';
  let saving = false;
  let errorMessage = '';
  let wasOpen = false;
  let providers: Provider[] = [];
  let providersLoading = false;
  let selectedProviderIds: string[] = [];
  let modelRules = '';

  $: if (open && !wasOpen) {
    wasOpen = true;
    name = '';
    createdKey = '';
    errorMessage = '';
    selectedProviderIds = [];
    modelRules = '';
    void loadProviders();
  } else if (!open) {
    wasOpen = false;
  }

  async function loadProviders(): Promise<void> {
    providersLoading = true;
    try {
      providers = await api.providers();
    } catch (error) {
      errorMessage = localizedError(error, 'Could not load providers.', tr);
    } finally {
      providersLoading = false;
    }
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    errorMessage = '';
    saving = true;
    try {
      const result = await api.createApiKey(name.trim(), {
        allowed_provider_ids: [...selectedProviderIds],
        allowed_models: parseModelRules(modelRules),
      });
      createdKey = result.key;
      name = '';
      onCreated();
    } catch (error) {
      errorMessage = localizedError(error, 'Could not create this API key.', tr);
    } finally {
      saving = false;
    }
  }

  function close(): void {
    createdKey = '';
    onClose();
  }
</script>

<ArkDialog {open} closeLabel={tr('Close dialog')} title={tr(createdKey ? 'Your new API key' : 'Create an API key')} kicker={tr('EXOROUTE CONTROL PLANE')} onClose={close}>
  {#if createdKey}
    <div class="modal-form"><p class="modal-description">{tr('Copy this key now. ExoRoute will only display the secret once.')}</p><div class="key-reveal-field"><span>{tr('Gateway API key')}</span><code>{createdKey}</code></div><div class="modal-actions"><ArkClipboard value={createdKey} copyLabel={tr('Copy key')} copiedLabel={tr('Copied!')} /><button class="primary-button" onclick={close}>{tr('Done')}</button></div></div>
  {:else}
    <form class="modal-form" onsubmit={submit}><p class="modal-description">{tr('Every request under /v1 requires an enabled client API key. Copy the generated key when it is shown; it cannot be viewed again.')}</p><ArkField label={tr('Key name')} bind:value={name} required placeholder={tr('e.g. local development')} /><ApiKeyScopeFields {tr} {providers} bind:selectedProviderIds bind:modelRules loadingProviders={providersLoading} disabled={saving} />{#if errorMessage}<div class="form-error" role="alert">{errorMessage}</div>{/if}<div class="modal-actions"><button type="button" class="secondary-button" onclick={close}>{tr('Cancel')}</button><button class="primary-button" disabled={saving}>{#if saving}<LoaderCircle size={15} class="spin" />{:else}<Plus size={15} />{/if}{tr('Create key')}</button></div></form>
  {/if}
</ArkDialog>
