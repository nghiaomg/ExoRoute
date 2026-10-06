<script lang="ts">
  import { Search, SlidersHorizontal, X } from '@lucide/svelte';
  import ArkField from '../../components/ArkField.svelte';
  import ArkSelect from '../../components/ArkSelect.svelte';
  import type { Translate } from '../../lib/format';
  import type { RequestLogFilters } from '../../lib/types';
  import { appliedRequestFilters, sameRequestFilters, type RequestFilterKey } from './request.metrics';

  export let tr: Translate;
  export let activeFilters: RequestLogFilters;
  export let loading: boolean;
  export let onApply: (filters: RequestLogFilters) => void;
  export let onClearFilters: () => void;
  export let onRemoveFilter: (key: RequestFilterKey) => void;

  // The draft lives here so typing never queries; it re-syncs whenever the
  // applied filters change (apply, clear, or a removed chip).
  let apiKey = activeFilters.api_key_id ?? '';
  let model = activeFilters.model ?? '';
  let providerId = activeFilters.provider_id ?? '';
  let status = activeFilters.status ?? '';
  let syncedFrom: RequestLogFilters | null = null;

  $: if (activeFilters !== syncedFrom) {
    syncedFrom = activeFilters;
    apiKey = activeFilters.api_key_id ?? '';
    model = activeFilters.model ?? '';
    providerId = activeFilters.provider_id ?? '';
    status = activeFilters.status ?? '';
  }

  $: draft = { api_key_id: apiKey, model, provider_id: providerId, status } as RequestLogFilters;
  $: dirty = !sameRequestFilters(draft, activeFilters);
  $: applied = appliedRequestFilters(activeFilters, tr);
  // Apply only earns screen space when the draft changed; Clear stays reachable
  // whenever something is applied.
  $: showActions = dirty || applied.length > 0;

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    if (!dirty) return;
    onApply(draft);
  }
</script>

<form class="request-filter-panel" onsubmit={submit}>
  <div class="request-filter-grid">
    <ArkField
      label={tr('API key ID')}
      bind:value={apiKey}
      maxlength={256}
      placeholder={tr('Filter by API key ID')}
      disabled={loading}
    />
    <ArkField
      label={tr('Requested model')}
      bind:value={model}
      maxlength={256}
      placeholder={tr('Exact model name')}
      disabled={loading}
    />
    <ArkField
      label={tr('Provider ID')}
      bind:value={providerId}
      maxlength={256}
      placeholder={tr('Exact provider ID')}
      disabled={loading}
    />
    <ArkSelect
      label={tr('Outcome')}
      bind:value={status}
      disabled={loading}
      items={[
        { label: tr('All outcomes'), value: '' },
        { label: tr('Successful'), value: 'success' },
        { label: tr('Failed'), value: 'failure' },
      ]}
    />
    {#if showActions}
      <div class="request-filter-actions">
        <button class="primary-button compact" type="submit" disabled={loading || !dirty}>
          <Search size={14} />{tr('Apply filters')}
        </button>
        <button class="secondary-button compact" type="button" disabled={loading} onclick={onClearFilters}>
          {tr('Clear')}
        </button>
      </div>
    {/if}
  </div>

  {#if applied.length}
    <div class="request-filter-chips" aria-label={tr('Applied filters')}>
      <span class="request-filter-chips-icon" aria-hidden="true"><SlidersHorizontal size={13} /></span>
      {#each applied as filter (filter.key)}
        <span class="request-filter-chip">
          <span class="request-filter-chip-label">{filter.label}</span>
          <span class="request-filter-chip-value" title={filter.value}>{filter.value}</span>
          <button
            type="button"
            class="request-filter-chip-remove"
            aria-label={tr('Remove {name}', { name: filter.label })}
            title={tr('Remove {name}', { name: filter.label })}
            disabled={loading}
            onclick={() => onRemoveFilter(filter.key)}
          >
            <X size={12} />
          </button>
        </span>
      {/each}
    </div>
  {/if}
</form>

<style>
  .request-filter-panel {
    position: sticky;
    top: 0;
    z-index: 6;
    margin: 0 0 18px;
    padding: 14px 16px;
    border: 1px solid #e2e4ed;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.96);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    box-shadow: 0 4px 20px rgba(15, 23, 42, 0.05);
    transition: box-shadow 0.16s ease, border-color 0.16s ease;
  }

  .request-filter-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(140px, 1fr)) auto;
    align-items: end;
    gap: 12px;
  }

  .request-filter-actions {
    display: flex;
    gap: 8px;
  }

  .request-filter-actions button {
    height: 38px;
  }

  .request-filter-chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid #f1f5f9;
  }

  .request-filter-chips-icon {
    display: grid;
    place-items: center;
    color: #64748b;
  }

  .request-filter-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    padding: 3px 6px 3px 10px;
    border: 1px solid #fed7aa;
    border-radius: 999px;
    background: #fffaf5;
    box-shadow: 0 1px 2px rgba(249, 115, 22, 0.05);
    font-size: 11px;
  }

  .request-filter-chip-label {
    color: #ea580c;
    font-weight: 700;
    white-space: nowrap;
  }

  .request-filter-chip-value {
    max-width: 160px;
    overflow: hidden;
    color: #0f172a;
    font-family: var(--font-mono);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .request-filter-chip-remove {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: 50%;
    color: #ea580c;
    background: transparent;
    cursor: pointer;
    transition: background 0.12s ease;
  }

  .request-filter-chip-remove:hover:not(:disabled) {
    background: #fed7aa;
  }

  .request-filter-chip-remove:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  :global(:root[data-theme='dark']) .request-filter-panel {
    border-color: rgba(255, 255, 255, 0.08);
    background: rgba(24, 25, 38, 0.95);
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.45);
  }

  :global(:root[data-theme='dark']) .request-filter-chips {
    border-top-color: #282a3c;
  }

  :global(:root[data-theme='dark']) .request-filter-chip {
    border-color: rgba(249, 115, 22, 0.3);
    background: #2b1d18;
  }

  :global(:root[data-theme='dark']) .request-filter-chip-label {
    color: #fed7aa;
  }

  :global(:root[data-theme='dark']) .request-filter-chip-value {
    color: #f0edff;
  }

  :global(:root[data-theme='dark']) .request-filter-chip-remove {
    color: #fed7aa;
  }

  :global(:root[data-theme='dark']) .request-filter-chip-remove:hover:not(:disabled) {
    background: #431f12;
  }

  /* Tablet (768px – 1050px) */
  @media (max-width: 1050px) and (min-width: 651px) {
    .request-filter-grid {
      grid-template-columns: repeat(2, minmax(140px, 1fr));
    }
    .request-filter-actions {
      grid-column: 1 / -1;
      justify-content: flex-start;
      margin-top: 4px;
    }
  }

  /* ─── Mobile App Style (<= 650px: 320px - 430px) ─── */
  @media (max-width: 650px) {
    .request-filter-panel {
      padding: 12px 14px;
      border-radius: 16px;
      margin-bottom: 14px;
    }
    .request-filter-grid {
      grid-template-columns: 1fr;
      gap: 10px;
    }
    .request-filter-actions {
      gap: 8px;
    }
    .request-filter-actions button {
      flex: 1;
      height: 42px;
      justify-content: center;
    }
    .request-filter-chip-value {
      max-width: 120px;
    }
  }

  /* ─── Ultra-compact Displays (320px - 360px) ─── */
  @media (max-width: 360px) {
    .request-filter-panel {
      padding: 10px 10px;
      gap: 8px;
    }
    .request-filter-chip-value {
      max-width: 96px;
    }
  }
</style>
