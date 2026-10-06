<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { LoaderCircle, Save, ShieldAlert } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import Toast from '../../components/Toast.svelte';
  import { localizedError } from '../../lib/errors';
  import type { Translate } from '../../lib/format';
  import type { GatewayApiKey, GatewayApiKeyScopeInput, Provider } from '../../lib/types';
  import ApiKeyScopeFields from './ApiKeyScopeFields.svelte';
  import { modelRulesText, parseModelRules } from './scope';

  export let apiKey: GatewayApiKey;
  export let tr: Translate;
  export let onUpdated: () => void;

  let providers: Provider[] = [];
  let providersLoading = true;
  let providersError = '';
  let selectedProviderIds: string[] = [];
  let modelRules = '';
  let saving = false;
  let saveError = '';
  let loadedKeyId = '';
  let toast: { tone: 'success' | 'error'; title: string; message?: string } | null = null;
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  function showToast(title: string): void {
    if (toastTimer) clearTimeout(toastTimer);
    toast = { tone: 'success', title };
    toastTimer = setTimeout(() => {
      toast = null;
      toastTimer = undefined;
    }, 4000);
  }

  function dismissToast(): void {
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = undefined;
    toast = null;
  }

  $: if (apiKey.id !== loadedKeyId) {
    loadedKeyId = apiKey.id;
    selectedProviderIds = [...(apiKey.allowed_provider_ids ?? [])];
    modelRules = modelRulesText(apiKey.allowed_models);
    saveError = '';
    dismissToast();
  }

  onDestroy(() => {
    if (toastTimer) clearTimeout(toastTimer);
  });

  onMount(async () => {
    try {
      providers = await api.providers();
    } catch (error) {
      providersError = localizedError(error, 'Could not load providers.', tr);
    } finally {
      providersLoading = false;
    }
  });

  async function save(): Promise<void> {
    saving = true;
    saveError = '';
    dismissToast();
    try {
      const scope: GatewayApiKeyScopeInput = {
        allowed_provider_ids: [...selectedProviderIds],
        allowed_models: parseModelRules(modelRules),
      };
      await api.updateApiKey(apiKey.id, scope);
      showToast(tr('Access scope saved.'));
      onUpdated();
    } catch (error) {
      saveError = localizedError(error, 'Could not save this key’s access scope.', tr);
    } finally {
      saving = false;
    }
  }
</script>

<section class="scope-editor" aria-labelledby="api-key-scope-heading">
  <header class="scope-editor-heading">
    <h2 id="api-key-scope-heading">{tr('Access scope')}</h2>
    <p>{tr('Restrict this key to specific providers and models.')}</p>
  </header>

  {#if apiKey.scope_valid === false}
    <p class="scope-alert" role="alert">
      <ShieldAlert size={14} />
      {tr('The stored access scope is invalid, so every request with this key is rejected. Save a scope to repair it.')}
    </p>
  {:else if providersError}
    <p class="scope-alert" role="alert">
      <ShieldAlert size={14} />
      {providersError}
    </p>
  {/if}

  <ApiKeyScopeFields
    {tr}
    {providers}
    bind:selectedProviderIds
    bind:modelRules
    loadingProviders={providersLoading}
    disabled={saving}
  />

  {#if saveError}<div class="form-error" role="alert">{saveError}</div>{/if}

  <div class="scope-actions">
    <button type="button" class="primary-button compact" disabled={saving} onclick={save}>
      {#if saving}<LoaderCircle size={14} class="spin" />{tr('Saving…')}{:else}<Save size={14} />{tr('Save access scope')}{/if}
    </button>
  </div>

  <Toast {toast} {tr} onDismiss={dismissToast} />
</section>

<style>
  .scope-editor {
    display: grid;
    gap: 12px;
    padding: 18px;
    border: 1px solid var(--line);
    border-radius: 13px;
    background: var(--paper);
  }
  .scope-editor-heading h2 {
    margin: 0 0 4px;
    color: var(--ink);
    font: 700 15px var(--font-heading);
  }
  .scope-editor-heading p {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  .scope-alert {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    margin: 0;
    padding: 9px 11px;
    color: #b45309;
    border: 1px solid rgba(180, 83, 9, 0.28);
    border-radius: 9px;
    background: rgba(249, 115, 22, 0.08);
    font-size: 12px;
    line-height: 1.45;
  }
  .scope-actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
