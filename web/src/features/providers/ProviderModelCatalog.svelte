<script lang="ts">
  import { onDestroy } from 'svelte';
  import { Cpu, Download, LoaderCircle, RefreshCw, Save } from '@lucide/svelte';
  import { Field } from '@ark-ui/svelte/field';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { ModelTestTarget, Provider, ProviderModelImportResult } from '../../lib/types';
  import { mergeImportedModels } from '../../lib/provider-model-import';
  import ProviderModelTests from './ProviderModelTests.svelte';

  type CatalogState = 'idle' | 'loading' | 'ready' | 'empty' | 'error';
  type ImportState = 'loading' | 'success' | 'empty' | 'unsupported' | 'truncated' | 'error';
  interface ImportFeedback { state: ImportState; message: string }

  export let provider: Provider;
  export let open = false;
  export let tr: Translate;
  export let onProviderChanged: () => void;

  let models: string[] = [];
  let manualModels: string[] = [];
  let catalogState: CatalogState = 'idle';
  let catalogError = '';
  let importFeedback: ImportFeedback | null = null;
  let generation = 0;
  let importSequence = 0;
  let loadedProviderId = '';
  let wasOpen = false;
  let manualModelDraft = '';
  let manualModelNotice = '';
  let manualModelNoticeTone: 'success' | 'error' = 'success';
  let savingManualModel = false;
  let deletingModel = '';

  $: modelTargets = models.map((model): ModelTestTarget => ({ model, provider_id: provider.id }));
  $: isOAuthProvider = provider.capabilities?.oauth_accounts === true;
  $: savedModelCount = catalogState === 'idle' || catalogState === 'loading' ? provider.model_count || 0 : models.length;
  $: if (open && (!wasOpen || loadedProviderId !== provider.id)) {
    wasOpen = true;
    loadedProviderId = provider.id;
    importFeedback = null;
    manualModelDraft = '';
    manualModelNotice = '';
    manualModelNoticeTone = 'success';
    void loadCatalog();
  } else if (!open && wasOpen) {
    wasOpen = false;
    generation += 1;
    importSequence += 1;
  }

  async function loadCatalog(): Promise<void> {
    const requestGeneration = ++generation;
    catalogState = 'loading';
    catalogError = '';
    try {
      const catalog = await api.providerModels(provider.id);
      if (requestGeneration !== generation) return;
      models = catalog.models;
      manualModels = catalog.manual_models ?? [];
      if (catalog.models.length || !isOAuthProvider) {
        catalogState = catalog.models.length ? 'ready' : 'empty';
        return;
      }

      importFeedback = { state: 'loading', message: tr('Importing models…') };
      const imported: ProviderModelImportResult = await api.importProviderModels(provider.id);
      if (requestGeneration !== generation) return;
      if (!imported.available || imported.models.length === 0) {
        models = [];
        manualModels = [];
        catalogState = 'empty';
        importFeedback = {
          state: imported.available ? 'empty' : 'unsupported',
          message: imported.available
            ? tr('No models were returned by this provider.')
            : tr('This provider does not support model listing.'),
        };
        return;
      }
      models = imported.models;
      manualModels = [];
      catalogState = 'ready';
      importFeedback = {
        state: imported.truncated ? 'truncated' : 'success',
        message: imported.truncated
          ? tr('Imported {count} models. The provider list was truncated.', { count: imported.models.length })
          : tr('Imported {count} models.', { count: imported.models.length }),
      };
      onProviderChanged();
    } catch (error) {
      if (requestGeneration !== generation) return;
      models = [];
      catalogState = 'error';
      importFeedback = { state: 'error', message: localizedError(error, 'Could not import models.', tr) };
      catalogError = localizedError(error, 'Could not load model catalog.', tr);
    }
  }

  async function importModels(): Promise<void> {
    const providerId = provider.id;
    const sequence = ++importSequence;
    const requestGeneration = generation;
    importFeedback = { state: 'loading', message: tr('Importing models…') };
    let previousModels = models;
    let knownManualModels = manualModels;
    try {
      try {
        const catalog = await api.providerModels(providerId);
        previousModels = catalog.models;
        knownManualModels = catalog.manual_models ?? knownManualModels;
        manualModels = knownManualModels;
        models = previousModels;
        catalogState = previousModels.length ? 'ready' : 'empty';
      } catch {
        // Keep the last successful list and continue the import attempt.
      }
      const result = await api.importProviderModels(providerId);
      if (sequence !== importSequence || requestGeneration !== generation) return;
      const nonAuthoritativeCatalog = provider.adapter_id === 'nvidia_nim'
        || provider.capabilities?.model_catalog_authoritative === false;
      const pruned = Array.isArray(result.pruned) ? result.pruned : [];
      const merged = mergeImportedModels({
        available: result.available,
        authoritative: !nonAuthoritativeCatalog,
        importedModels: result.models,
        previousModels,
        manualModels: knownManualModels,
        pruned,
      });
      models = merged.models;
      manualModels = merged.manualModels;
      catalogState = merged.models.length ? 'ready' : 'empty';
      catalogError = '';
      const keptManual = typeof result.manual_kept === 'number' ? result.manual_kept : 0;
      const state: ImportState = !result.available ? 'unsupported' : result.truncated ? 'truncated' : result.models.length ? 'success' : 'empty';
      const message = state === 'unsupported'
        ? tr('This provider does not support model listing.')
        : state === 'truncated'
          ? tr('Imported {count} models. The provider list was truncated.', { count: result.models.length })
          : pruned.length
          ? tr('Imported {count} models. Removed {removed} unsupported saved models: {models}', { count: result.models.length, removed: pruned.length, models: pruned.join(', ') })
          : keptManual > 0
            ? tr('Imported {count} models. Kept {kept} manual models.', { count: result.models.length, kept: keptManual })
            : state === 'empty'
              ? tr('No models were returned by this provider.')
              : tr('Imported {count} models.', { count: result.models.length });
      importFeedback = { state, message };
      onProviderChanged();
    } catch (error) {
      if (sequence !== importSequence || requestGeneration !== generation) return;
      importFeedback = { state: 'error', message: localizedError(error, 'Could not import models.', tr) };
    }
  }

  async function saveManualModel(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (savingManualModel) return;
    const model = manualModelDraft.trim();
    if (!model) {
      manualModelNotice = tr('Provider model ID is required.');
      manualModelNoticeTone = 'error';
      return;
    }
    manualModelNotice = '';
    savingManualModel = true;
    try {
      const result = await api.addManualProviderModel(provider.id, model);
      if (!models.includes(result.model)) models = [...models, result.model].sort((left, right) => left.localeCompare(right));
      if (!manualModels.includes(result.model)) manualModels = [...manualModels, result.model].sort((left, right) => left.localeCompare(right));
      catalogState = 'ready';
      manualModelDraft = '';
      manualModelNotice = tr('Model saved.');
      manualModelNoticeTone = 'success';
      onProviderChanged();
    } catch (error) {
      manualModelNotice = localizedError(error, 'Could not save model.', tr);
      manualModelNoticeTone = 'error';
    } finally {
      savingManualModel = false;
    }
  }

  async function deleteModel(model: string): Promise<void> {
    const providerId = provider.id;
    deletingModel = model;
    try {
      await api.deleteProviderModel(providerId, model);
      if (provider.id !== providerId) return;
      models = models.filter((item) => item !== model);
      manualModels = manualModels.filter((item) => item !== model);
      catalogState = models.length ? 'ready' : 'empty';
      importFeedback = null;
      manualModelNotice = tr('Model deleted.');
      manualModelNoticeTone = 'success';
      onProviderChanged();
    } finally {
      deletingModel = '';
    }
  }

  onDestroy(() => {
    generation += 1;
    importSequence += 1;
  });
</script>

<section class="provider-model-section">
  <div class="provider-model-section-heading">
    <div><h3>{tr('Saved models')}</h3><p>{tr('Manage saved models for this provider. Import a list, add a model ID manually, or test models below.')}</p></div>
    <button class="primary-button compact" disabled={importFeedback?.state === 'loading' || deletingModel !== ''} onclick={importModels}>
      {#if importFeedback?.state === 'loading'}<LoaderCircle size={13} class="spin" />{tr('Importing models…')}
      {:else}<Download size={13} />{tr('Import models')}{/if}
    </button>
  </div>

  <form class="provider-manual-model-form" onsubmit={saveManualModel}>
    <Field.Root class="provider-manual-model-field">
      <Field.Label class="ark-field-label">{tr('Model ID')}</Field.Label>
      <div class="provider-manual-model-row">
        <Field.Input
          bind:value={manualModelDraft}
          class="ark-field-input"
          maxlength={256}
          placeholder={tr(provider.adapter_id === 'antigravity' ? 'e.g. ag/model-id' : 'e.g. provider/model-id')}
          aria-label={tr('Model ID')}
        />
        <button type="submit" class="secondary-button compact" disabled={savingManualModel || deletingModel !== ''}>
          {#if savingManualModel}<LoaderCircle size={13} class="spin" />{:else}<Save size={13} />{/if}
          <span>{tr('Save model')}</span>
        </button>
      </div>
      <Field.HelperText class="provider-manual-model-help">{tr('Add a model ID manually when provider discovery is unavailable.')}</Field.HelperText>
    </Field.Root>
  </form>
  {#if manualModelNotice}<div class="provider-import-feedback" class:success={manualModelNoticeTone === 'success'} class:error={manualModelNoticeTone === 'error'} role={manualModelNoticeTone === 'error' ? 'alert' : 'status'}>{manualModelNotice}</div>{/if}

  {#if importFeedback}<div class="provider-import-feedback" class:success={importFeedback.state === 'success'} class:warning={['empty', 'unsupported', 'truncated'].includes(importFeedback.state)} class:error={importFeedback.state === 'error'} role="status">{importFeedback.message}</div>{/if}

  {#if catalogState === 'loading'}
    <div class="inline-loading"><LoaderCircle size={16} class="spin" /><span>{tr('Loading saved models…')}</span></div>
  {:else if catalogState === 'error'}
    <div class="provider-model-empty" role="alert"><p>{catalogError}</p><button class="secondary-button compact" onclick={loadCatalog}><RefreshCw size={13} />{tr('Retry loading models')}</button></div>
  {:else if catalogState === 'empty'}
    <div class="provider-model-empty"><Cpu size={19} /><p>{tr('No saved models for this provider. Import a list or add a model ID manually.')}</p><button class="secondary-button compact" disabled={importFeedback?.state === 'loading' || deletingModel !== ''} onclick={importModels}><Download size={13} />{tr('Import models')}</button></div>
  {:else if catalogState === 'ready'}
    <ProviderModelTests provider={provider} models={modelTargets} {manualModels} {tr} onDeleteModel={deleteModel} />
  {/if}
</section>
