<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { Download, LoaderCircle, Search, SlidersHorizontal, X } from '@lucide/svelte';
  import SelectSearch from '../../components/SelectSearch.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import SelectOptions from '../../components/SelectOptions.svelte';
  import type { Translate } from '../../lib/format';
  import type { RequestLogFilters } from '../../lib/types';
  import { appliedRequestFilters, sameRequestFilters, type RequestFilterKey } from './request.metrics';
  import {
    createRequestFilterOptions,
    withPinnedOption,
    type RequestFilterOption,
    type RequestFilterOptions,
  } from './request.filters';

  export let tr: Translate;
  export let activeFilters: RequestLogFilters;
  export let loading: boolean;
  export let exporting: boolean;
  export let onApply: (filters: RequestLogFilters) => void;
  export let onClearFilters: () => void;
  export let onRemoveFilter: (key: RequestFilterKey) => void;
  export let onExport: () => void;

  // The gateway's marker for a request that carried no key. The id field is a
  // select now, so this filter value needs its own entry to stay reachable.
  const UNKNOWN_KEY_FILTER = 'unknown';

  // The draft lives here so typing never queries; it re-syncs whenever the
  // applied filters change (apply, clear, or a removed chip).
  let apiKey = activeFilters.api_key_id ?? '';
  let model = activeFilters.model ?? '';
  let providerId = activeFilters.provider_id ?? '';
  let status = activeFilters.status ?? '';
  let syncedFrom: RequestLogFilters | null = null;

  const EMPTY_OPTIONS: RequestFilterOptions = {
    apiKeyOptions: [],
    providerOptions: [],
    apiKeysLoading: false,
    apiKeysError: '',
    providersError: '',
    apiKeyLabel: (value) => value,
    providerLabel: (value) => value,
  };
  let filterOptions = EMPTY_OPTIONS;
  // The query the API-key option list currently answers, so the "no key
  // recorded" entry can stand aside while the user is searching.
  let apiKeyQuery = '';

  // The panel owns the option lists: they describe this form's two id fields and
  // nothing else on the page reads them.
  const optionSource = createRequestFilterOptions({
    tr,
    onChange: (next) => {
      filterOptions = next;
    },
  });

  onMount(() => optionSource.loadProviders());
  onDestroy(() => optionSource.destroy());

  $: if (activeFilters !== syncedFrom) {
    syncedFrom = activeFilters;
    apiKey = activeFilters.api_key_id ?? '';
    model = activeFilters.model ?? '';
    providerId = activeFilters.provider_id ?? '';
    status = activeFilters.status ?? '';
  }

  // The "no key recorded" entry and a pinned selection can repeat an id.
  function dedupeOptions(options: RequestFilterOption[]): RequestFilterOption[] {
    const seen = new Set<string>();
    const unique: RequestFilterOption[] = [];
    for (const option of options) {
      if (seen.has(option.value)) continue;
      seen.add(option.value);
      unique.push(option);
    }
    return unique;
  }

  function apiKeyLabelFor(value: string): string {
    if (value === UNKNOWN_KEY_FILTER) return tr('Unknown key');
    return filterOptions.apiKeyLabel(value);
  }

  function searchApiKeys(query: string): void {
    apiKeyQuery = query;
    optionSource.searchApiKeys(query);
  }

  $: draft = { api_key_id: apiKey, model, provider_id: providerId, status } as RequestLogFilters;
  $: dirty = !sameRequestFilters(draft, activeFilters);
  $: applied = appliedRequestFilters(activeFilters, tr, {
    api_key_id: apiKeyLabelFor(activeFilters.api_key_id ?? ''),
    provider_id: filterOptions.providerLabel(activeFilters.provider_id ?? ''),
  });
  // Apply only earns screen space when the draft changed; Clear stays reachable
  // whenever something is applied.
  $: showActions = dirty || applied.length > 0;

  // The "no key recorded" entry keeps that filter reachable now the field is a
  // select. It stands aside while the key list is searching, failed, or being
  // searched, so the field's empty slot can report what is happening instead of
  // showing this one option and hiding it.
  $: showUnknownKeyOption =
    !apiKeyQuery.trim() && !filterOptions.apiKeysLoading && !filterOptions.apiKeysError;
  $: apiKeyItems = dedupeOptions([
    ...(showUnknownKeyOption ? [{ label: tr('Unknown key'), value: UNKNOWN_KEY_FILTER }] : []),
    ...withPinnedOption(filterOptions.apiKeyOptions, apiKey, apiKeyLabelFor(apiKey)),
  ]);
  $: providerItems = withPinnedOption(
    filterOptions.providerOptions,
    providerId,
    filterOptions.providerLabel(providerId),
  );
  $: apiKeyEmptyText = filterOptions.apiKeysLoading
    ? tr('Searching API keys…')
    : filterOptions.apiKeysError || tr('No matching API keys');
  $: providerEmptyText = filterOptions.providersError || tr('No matching providers');

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    if (!dirty) return;
    onApply(draft);
  }
</script>

<form class="request-filter-panel" onsubmit={submit}>
  <div class="request-filter-grid">
    <SelectSearch
      label={tr('API key ID')}
      clearLabel={tr('Clear selection')}
      items={apiKeyItems}
      value={apiKey}
      placeholder={tr('Search API keys by name or ID')}
      noOptionsText={apiKeyEmptyText}
      disabled={loading}
      onSearch={searchApiKeys}
      onValueChange={(next) => (apiKey = next)}
    />
    <SelectSearch
      label={tr('Provider ID')}
      clearLabel={tr('Clear selection')}
      items={providerItems}
      value={providerId}
      placeholder={tr('Search providers by name or ID')}
      noOptionsText={providerEmptyText}
      disabled={loading}
      onValueChange={(next) => (providerId = next)}
    />
    <ArkField
      label={tr('Requested model')}
      bind:value={model}
      maxlength={256}
      placeholder={tr('Exact model name')}
      disabled={loading}
    />
    <SelectOptions
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
        <Button class="primary-button compact" type="submit" disabled={loading || !dirty}>
          <Search size={14} />{tr('Apply filters')}
        </Button>
        <Button class="secondary-button compact" type="button" disabled={loading} onclick={onClearFilters}>
          {tr('Clear')}
        </Button>
      </div>
    {/if}
    <div class="request-filter-actions">
      <Button
        class="secondary-button compact"
        type="button"
        disabled={loading || exporting}
        onclick={onExport}
      >
        {#if exporting}
          <LoaderCircle size={13} class="spin" />{tr('Exporting requests…')}
        {:else}
          <Download size={13} />{tr('Export CSV')}
        {/if}
      </Button>
    </div>
  </div>

  {#if applied.length}
    <div class="request-filter-chips" aria-label={tr('Applied filters')}>
      <span class="request-filter-chips-icon" aria-hidden="true"><SlidersHorizontal size={13} /></span>
      {#each applied as filter (filter.key)}
        <span class="request-filter-chip">
          <span class="request-filter-chip-label">{filter.label}</span>
          <span class="request-filter-chip-value" title={filter.title ?? filter.value}>{filter.value}</span>
          <Button
            type="button"
            class="request-filter-chip-remove"
            aria-label={tr('Remove {name}', { name: filter.label })}
            title={tr('Remove {name}', { name: filter.label })}
            disabled={loading}
            onclick={() => onRemoveFilter(filter.key)}
          >
            <X size={12} />
          </Button>
        </span>
      {/each}
    </div>
  {/if}
</form>

<style>
  /* Soft Neo-Brutalism frame, like every other card in the light theme. The
     panel used to be position: sticky here, but once the page column scrolls
     (it does on laptop-height displays now that every section keeps its real
     height) the pinned panel let the table card slide underneath it, and the
     two framed cards rendered stacked on top of each other. As an ordinary
     flex sibling of the table card nothing can overlap it; the table card
     keeps its own sticky header, so scanning rows loses nothing. */
  .request-filter-panel {
    margin: 0 0 18px;
    padding: 14px 16px;
    border: var(--nb-border);
    border-radius: 12px;
    background: var(--surface-raised);
    box-shadow: var(--nb-shadow-md);
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

  .request-filter-actions :global(.shared-button) {
    height: 38px;
  }

  .request-filter-chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }

  .request-filter-chips-icon {
    display: grid;
    place-items: center;
    color: var(--muted);
  }

  /* Chips sit inside the framed panel, so they stay flat and keep the light
     orange-tint stroke the theme uses for small badges. */
  .request-filter-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    padding: 3px 6px 3px 10px;
    border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--line));
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 7%, var(--surface));
    box-shadow: none;
    font-size: 11px;
  }

  .request-filter-chip-label {
    color: var(--accent-strong);
    font-weight: 700;
    white-space: nowrap;
  }

  .request-filter-chip-value {
    max-width: 160px;
    overflow: hidden;
    color: var(--ink);
    font-family: var(--font-mono);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  :global(.request-filter-chip-remove) {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: 50%;
    color: var(--accent-strong);
    background: transparent;
    cursor: pointer;
    transition: background 0.12s ease;
  }

  :global(.request-filter-chip-remove:hover:not(:disabled)) {
    background: color-mix(in srgb, var(--accent) 30%, var(--surface-raised));
  }

  :global(.request-filter-chip-remove:disabled) {
    opacity: 0.5;
    cursor: not-allowed;
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
    .request-filter-actions :global(.shared-button) {
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
