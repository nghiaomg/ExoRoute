<script lang="ts">
  import { onDestroy } from 'svelte';
  import { Portal } from '@ark-ui/svelte/portal';
  import { Check, Copy, Cpu, LoaderCircle, Search, Trash2, X, Zap } from '@lucide/svelte';
  import ArkCheckbox from '../../components/ArkCheckbox.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { ModelTestResult, ModelTestTarget, Provider } from '../../lib/types';

  type ModelTestView = ModelTestResult & { phase: 'pending' | 'passed' | 'failed' };
  interface ModelTestToast { tone: 'success' | 'error'; title: string; message: string }

  export let provider: Provider;
  export let models: ModelTestTarget[];
  export let tr: Translate;
  export let onDeleteModel: (model: string) => Promise<void>;

  const INITIAL_RENDER_COUNT = 48;
  const CHUNK_SIZE = 36;

  let query = '';
  let modelTestStates: Record<string, ModelTestView> = {};
  let selectedModelKeys: string[] = [];
  let displayLimit = INITIAL_RENDER_COUNT;
  let listContainer: HTMLElement | null = null;
  let scrollTicking = false;
  let toast: ModelTestToast | null = null;
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let copiedModelKey = '';
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let deletingModelKey = '';
  let confirmingDeleteKey = '';
  let confirmDeleteTimer: ReturnType<typeof setTimeout> | undefined;

  $: modelPrefix = provider?.model_prefix || provider?.id || '';

  $: pruneRemovedModels(models);
  $: filteredModels = models.filter((item) => item.model.toLowerCase().includes(query.toLowerCase()));

  // Reset pagination on query or models update
  $: if (query !== undefined || models) {
    displayLimit = INITIAL_RENDER_COUNT;
  }

  $: visibleModels = filteredModels.slice(0, displayLimit);
  $: visibleKeys = visibleModels.map(modelTestKey);
  $: selectedKeysSet = new Set(selectedModelKeys);
  $: selectedVisibleCount = visibleKeys.filter((key) => selectedKeysSet.has(key)).length;
  $: allVisibleSelected = visibleKeys.length > 0 && selectedVisibleCount === visibleKeys.length;
  $: selectedPendingCount = models.filter((target) => selectedKeysSet.has(modelTestKey(target)) && modelTestStates[modelTestKey(target)]?.phase === 'pending').length;
  $: selectedTestableCount = selectedModelKeys.length - selectedPendingCount;

  function modelTestKey(target: ModelTestTarget): string {
    return `${target.provider_id}:${target.model}`;
  }

  function loadMore(): void {
    if (displayLimit < filteredModels.length) {
      displayLimit = Math.min(displayLimit + CHUNK_SIZE, filteredModels.length);
    }
  }

  function handleScroll(event: Event): void {
    if (scrollTicking) return;
    scrollTicking = true;
    requestAnimationFrame(() => {
      scrollTicking = false;
      const target = event.target as HTMLElement;
      if (!target) return;
      if (target.scrollTop + target.clientHeight >= target.scrollHeight - 350) {
        loadMore();
      }
    });
  }

  function setupObserver(node: HTMLElement) {
    if (typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver((entries) => {
      if (entries[0]?.isIntersecting) {
        loadMore();
      }
    }, {
      root: listContainer,
      rootMargin: '350px',
    });
    observer.observe(node);
    return {
      destroy() {
        observer.disconnect();
      },
    };
  }

  function pruneRemovedModels(currentModels: ModelTestTarget[]): void {
    const allowedKeys = new Set(currentModels.map(modelTestKey));
    const selected = selectedModelKeys.filter((key) => allowedKeys.has(key));
    if (selected.length !== selectedModelKeys.length) selectedModelKeys = selected;
    const states = Object.fromEntries(Object.entries(modelTestStates).filter(([key]) => allowedKeys.has(key)));
    if (Object.keys(states).length !== Object.keys(modelTestStates).length) modelTestStates = states;
  }

  function toggleModel(target: ModelTestTarget, selected: boolean): void {
    const key = modelTestKey(target);
    selectedModelKeys = selected
      ? [...new Set([...selectedModelKeys, key])]
      : selectedModelKeys.filter((item) => item !== key);
  }

  function toggleVisibleModels(selected: boolean): void {
    const visible = new Set(visibleKeys);
    selectedModelKeys = selected
      ? [...new Set([...selectedModelKeys, ...visible])]
      : selectedModelKeys.filter((key) => !visible.has(key));
  }

  function showToast(tone: ModelTestToast['tone'], title: string, message: string): void {
    if (toastTimer) clearTimeout(toastTimer);
    toast = { tone, title, message };
    toastTimer = setTimeout(() => {
      toast = null;
      toastTimer = undefined;
    }, 6000);
  }

  function dismissToast(): void {
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = undefined;
    toast = null;
  }

  async function testModels(requestedTargets: ModelTestTarget[]): Promise<void> {
    const uniqueTargets = [...new Map(requestedTargets.map((target) => [modelTestKey(target), target])).values()];
    const pendingTargets = uniqueTargets.filter((target) => modelTestStates[modelTestKey(target)]?.phase !== 'pending');
    if (!pendingTargets.length) return;
    modelTestStates = {
      ...modelTestStates,
      ...Object.fromEntries(pendingTargets.map((target) => [modelTestKey(target), { ...target, test_passed: false, latency_ms: 0, phase: 'pending' as const }])),
    };

    const outcomes: ModelTestResult[] = [];
    for (let offset = 0; offset < pendingTargets.length; offset += 32) {
      const batch = pendingTargets.slice(offset, offset + 32);
      try {
        const response = await api.testModels(batch);
        const byKey = new Map(response.results.map((result) => [modelTestKey(result), result]));
        const nextStates = { ...modelTestStates };
        for (const target of batch) {
          const result = byKey.get(modelTestKey(target)) ?? { ...target, test_passed: false, status: null, latency_ms: 0, message: tr('No test result was returned for this model.') };
          outcomes.push(result);
          nextStates[modelTestKey(target)] = { ...result, phase: result.test_passed ? 'passed' : 'failed' };
        }
        modelTestStates = nextStates;
      } catch (error) {
        const message = localizedError(error, 'Could not test models.', tr);
        const nextStates = { ...modelTestStates };
        for (const target of batch) {
          const result: ModelTestResult = { ...target, test_passed: false, status: null, latency_ms: 0, message };
          outcomes.push(result);
          nextStates[modelTestKey(target)] = { ...result, phase: 'failed' };
        }
        modelTestStates = nextStates;
      }
    }

    const passed = outcomes.filter((result) => result.test_passed).length;
    const failed = outcomes.length - passed;
    if (outcomes.length === 1) {
      const [result] = outcomes;
      const details = [tr('Tested in {duration} ms', { duration: result.latency_ms }), result.status != null ? tr('HTTP {status}', { status: result.status }) : '', result.message ?? '', result.provider_response_body ?? ''].filter(Boolean).join(' · ');
      showToast(result.test_passed ? 'success' : 'error', tr(result.test_passed ? 'Model test passed' : 'Model test failed'), `${provider.name} · ${result.model}${details ? ` · ${details}` : ''}`);
      return;
    }
    const failures = outcomes.filter((result) => !result.test_passed).slice(0, 2);
    const failureDetails = failures.map((result) => `${provider.name} · ${result.model}${result.status != null ? ` (${result.status})` : ''}`).join('; ');
    const summary = tr('{passed} passed, {failed} failed out of {count} models.', { passed, failed, count: outcomes.length });
    showToast(failed ? 'error' : 'success', tr(failed ? 'Model tests completed with failures' : 'All model tests passed'), failureDetails ? `${summary} ${failureDetails}` : summary);
  }

  function testSelected(): void {
    const selected = new Set(selectedModelKeys);
    void testModels(models.filter((target) => selected.has(modelTestKey(target))));
  }

  function clearConfirmDeleteTimer(): void {
    if (confirmDeleteTimer) {
      clearTimeout(confirmDeleteTimer);
      confirmDeleteTimer = undefined;
    }
  }

  async function handleDeleteClick(target: ModelTestTarget, key: string): Promise<void> {
    if (deletingModelKey) return;

    if (confirmingDeleteKey !== key) {
      clearConfirmDeleteTimer();
      confirmingDeleteKey = key;
      confirmDeleteTimer = setTimeout(() => {
        if (confirmingDeleteKey === key) {
          confirmingDeleteKey = '';
        }
      }, 3500);
      return;
    }

    clearConfirmDeleteTimer();
    confirmingDeleteKey = '';
    deletingModelKey = key;
    try {
      await onDeleteModel(target.model);
      showToast('success', tr('Model deleted.'), target.model);
    } catch (error) {
      showToast('error', tr('Could not delete model.'), localizedError(error, 'Could not delete model.', tr));
    } finally {
      if (deletingModelKey === key) deletingModelKey = '';
    }
  }

  async function copyModelId(modelName: string, key: string): Promise<void> {
    const formatted = `${modelPrefix}/${modelName}`;
    try {
      await navigator.clipboard.writeText(formatted);
      copiedModelKey = key;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        if (copiedModelKey === key) copiedModelKey = '';
      }, 2000);
    } catch {
      // Clipboard fallback
    }
  }

  onDestroy(() => {
    if (toastTimer) clearTimeout(toastTimer);
    if (copyTimer) clearTimeout(copyTimer);
    clearConfirmDeleteTimer();
  });
</script>

<div class="provider-model-filter">
  <label class="search-box"><Search size={15} /><input bind:value={query} placeholder={tr('Search saved models')} aria-label={tr('Search saved models')} /></label>
  <span class="provider-model-count">
    {#if query.trim()}
      {filteredModels.length} / {models.length} {tr('saved models')}
    {:else}
      {models.length} {tr('saved models')}
    {/if}
  </span>
</div>

{#if filteredModels.length}
  <div class="model-test-toolbar">
    <div class="model-test-toolbar-select">
      <ArkCheckbox checked={selectedVisibleCount === 0 ? false : allVisibleSelected ? true : 'indeterminate'} label={tr('Select all visible models')} onCheckedChange={(checked) => toggleVisibleModels(checked === true)} />
    </div>
    {#if selectedModelKeys.length}
      <div class="model-test-toolbar-actions">
        <span class="model-test-selected-count">{tr('{count} selected', { count: selectedModelKeys.length })}</span>
        <button class="primary-button compact" disabled={selectedTestableCount === 0} onclick={testSelected}>
          <Zap size={13} />
          <span>{tr('Test selected ({count})', { count: selectedModelKeys.length })}</span>
        </button>
        <button class="text-button compact" onclick={() => selectedModelKeys = []}>
          {tr('Clear selection')}
        </button>
      </div>
    {/if}
  </div>
  <div class="provider-model-list" bind:this={listContainer} onscroll={handleScroll}>
    {#each visibleModels as model (modelTestKey(model))}
      {@const key = modelTestKey(model)}
      {@const testState = modelTestStates[key]}
      {@const isCopied = copiedModelKey === key}
      {@const formattedId = `${modelPrefix}/${model.model}`}
      {@const isConfirming = confirmingDeleteKey === key}
      {@const isDeleting = deletingModelKey === key}
      <article class="provider-model-card" class:selected={selectedKeysSet.has(key)}>
        <div class="model-card-header">
          <ArkCheckbox
            checked={selectedKeysSet.has(key)}
            disabled={deletingModelKey !== ''}
            ariaLabel={tr('Select {model} from {provider}', { model: model.model, provider: provider.name })}
            onCheckedChange={(checked) => toggleModel(model, checked === true)}
          />
          <div class="model-card-header-actions">
            <button
              type="button"
              class="model-copy-btn"
              class:copied={isCopied}
              title={isCopied ? tr('Model ID copied') : tr('Copy {format}', { format: formattedId })}
              aria-label={isCopied ? tr('Model ID copied') : tr('Copy {format}', { format: formattedId })}
              onclick={(e) => { e.stopPropagation(); copyModelId(model.model, key); }}
            >
              {#if isCopied}
                <Check size={13} class="copy-success-icon" />
              {:else}
                <Copy size={13} />
              {/if}
            </button>
            <button
              type="button"
              class="row-icon danger-hover"
              class:danger-confirm={isConfirming}
              disabled={deletingModelKey !== '' || testState?.phase === 'pending'}
              title={isConfirming
                ? tr('Click again to delete “{model}”', { model: model.model })
                : tr('Delete model “{model}”', { model: model.model })}
              aria-label={isConfirming
                ? tr('Click again to delete “{model}”', { model: model.model })
                : tr('Delete model “{model}”', { model: model.model })}
              onclick={(e) => { e.stopPropagation(); void handleDeleteClick(model, key); }}
            >
              {#if isDeleting}
                <LoaderCircle size={13} class="spin" />
              {:else if isConfirming}
                <Check size={14} />
              {:else}
                <Trash2 size={14} />
              {/if}
            </button>
          </div>
        </div>

        <div class="model-card-info">
          <div class="model-title-wrap">
            <Cpu size={14} class="model-icon" />
            <strong class="model-name" title={model.model}>{model.model}</strong>
          </div>
          <code class="model-routed-code" title={formattedId}>{formattedId}</code>
        </div>

        <div class="model-card-footer">
          <button
            type="button"
            class="secondary-button compact model-test-btn"
            disabled={testState?.phase === 'pending'}
            onclick={() => testModels([model])}
          >
            {#if testState?.phase === 'pending'}
              <LoaderCircle size={12} class="spin" />
              <span>{tr('Testing…')}</span>
            {:else}
              <Zap size={12} />
              <span>{tr('Test')}</span>
            {/if}
          </button>

          {#if testState?.phase === 'passed'}
            <span class="model-test-chip success" title={testState.message ?? ''}>
              <Check size={11} />
              <span>{testState.latency_ms ?? 0}ms</span>
            </span>
          {:else if testState?.phase === 'failed'}
            <span class="model-test-chip error" title={[testState.message ?? '', testState.provider_response_body ?? ''].filter(Boolean).join(' · ')}>
              <X size={11} />
              <span>{testState.status != null ? `HTTP ${testState.status}` : tr('Test failed')}</span>
            </span>
          {:else}
            <span class="state-label enabled">
              <i></i>{tr('Available')}
            </span>
          {/if}
        </div>
      </article>
    {/each}

    {#if displayLimit < filteredModels.length}
      <div use:setupObserver class="models-scroll-sentinel">
        <LoaderCircle size={14} class="spin" />
        <span>{tr('Showing {count} of {total} models', { count: visibleModels.length, total: filteredModels.length })}</span>
        <button
          type="button"
          class="secondary-button compact"
          onclick={() => { displayLimit = filteredModels.length; }}
        >
          {tr('Show all')}
        </button>
      </div>
    {/if}
  </div>
{:else}
  <EmptyState icon="search" title={tr('No matching models')} description={tr('Try a different search, or clear the filter.')} />
{/if}

{#if toast}
  <Portal>
    <div class="model-test-toast" class:success={toast.tone === 'success'} class:error={toast.tone === 'error'} role="status">
      <span class="model-test-toast-mark">
        {#if toast.tone === 'success'}<Check size={15} />{:else}<X size={15} />{/if}
      </span>
      <div class="model-test-toast-copy">
        <strong>{toast.title}</strong>
        <small>{toast.message}</small>
      </div>
      <button class="model-test-toast-dismiss" aria-label={tr('Dismiss notification')} onclick={dismissToast}>
        <X size={14} />
      </button>
    </div>
  </Portal>
{/if}
