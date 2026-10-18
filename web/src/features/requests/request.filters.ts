//! Option sources for the requests filter fields.
//!
//! The API-key field searches on the server: `/api-keys` pages at 50 rows and a
//! key list has no cap, so filtering a loaded page in the browser would quietly
//! hide keys. `/providers` returns the whole list, so that field filters the
//! loaded options locally and this module only loads them once.

import { api } from '../../lib/api';
import { localizedError } from '../../lib/errors';
import type { Translate } from '../../lib/format';
import type { GatewayApiKey, Provider } from '../../lib/types';

/// Structurally `ComboboxOption` from `components/ArkCombobox.svelte`; declared
/// here so this module stays free of component imports and remains loadable by
/// the plain-node tests.
export type RequestFilterOption = {
  label: string;
  value: string;
  hint?: string;
  keywords?: string;
};

/// Typing is coalesced before the key search reaches the network.
const API_KEY_SEARCH_DEBOUNCE_MS = 250;

/// Names are kept so an applied chip still reads by name after a later search
/// replaced the option list. The API-key list is unbounded, so the memory is
/// bounded instead of the list.
const REMEMBERED_OPTION_LIMIT = 200;

export function apiKeyFilterOption(key: GatewayApiKey): RequestFilterOption {
  return { label: key.name, value: key.id, hint: key.id, keywords: key.id };
}

export function providerFilterOption(provider: Provider): RequestFilterOption {
  return { label: provider.name, value: provider.id, hint: provider.id, keywords: provider.id };
}

/// Remembers one option by value and evicts the oldest entries past `limit`.
export function rememberOption(
  remembered: ReadonlyMap<string, RequestFilterOption>,
  option: RequestFilterOption,
  limit = REMEMBERED_OPTION_LIMIT,
): Map<string, RequestFilterOption> {
  const next = new Map(remembered);
  next.delete(option.value);
  next.set(option.value, option);
  while (next.size > limit) {
    const oldest = next.keys().next().value;
    if (oldest === undefined) break;
    next.delete(oldest);
  }
  return next;
}

/// Prepends the chosen value when the freshly loaded options no longer hold it.
/// Ark reads the field text from the selected item, so without this a selection
/// that a later search filtered out would blank the input.
export function withPinnedOption(
  options: RequestFilterOption[],
  value: string,
  label: string,
): RequestFilterOption[] {
  if (!value || options.some((option) => option.value === value)) return options;
  return [{ label: label || value, value, hint: value }, ...options];
}

export type RequestFilterOptions = {
  apiKeyOptions: RequestFilterOption[];
  providerOptions: RequestFilterOption[];
  apiKeysLoading: boolean;
  apiKeysError: string;
  providersError: string;
  /// Display name for an applied id, falling back to the id itself.
  apiKeyLabel: (value: string) => string;
  providerLabel: (value: string) => string;
};

export type RequestFilterOptionsController = {
  /// Debounced; safe to call on every keystroke.
  searchApiKeys: (query: string) => void;
  loadProviders: () => void;
  destroy: () => void;
};

type RequestFilterOptionsConfig = {
  tr: Translate;
  onChange: (options: RequestFilterOptions) => void;
};

/// Loads the options behind the two id filter fields. The caller owns the state
/// it is handed and re-renders from each `onChange` snapshot.
export function createRequestFilterOptions(
  config: RequestFilterOptionsConfig,
): RequestFilterOptionsController {
  const { tr } = config;
  let rememberedApiKeys = new Map<string, RequestFilterOption>();
  let rememberedProviders = new Map<string, RequestFilterOption>();
  let apiKeyOptions: RequestFilterOption[] = [];
  let providerOptions: RequestFilterOption[] = [];
  let apiKeysLoading = false;
  let apiKeysError = '';
  let providersError = '';
  let apiKeyGeneration = 0;
  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  let activeController: AbortController | null = null;
  let destroyed = false;

  function emit(): void {
    config.onChange({
      apiKeyOptions,
      providerOptions,
      apiKeysLoading,
      apiKeysError,
      providersError,
      apiKeyLabel: (value: string) => rememberedApiKeys.get(value)?.label ?? value,
      providerLabel: (value: string) => rememberedProviders.get(value)?.label ?? value,
    });
  }

  function cancelSearch(): void {
    if (searchTimer !== null) clearTimeout(searchTimer);
    searchTimer = null;
    activeController?.abort();
    activeController = null;
  }

  async function fetchApiKeys(query: string): Promise<void> {
    const generation = ++apiKeyGeneration;
    cancelSearch();
    const controller = new AbortController();
    activeController = controller;
    apiKeysLoading = true;
    apiKeysError = '';
    emit();
    try {
      const page = await api.apiKeys({ q: query }, controller.signal);
      if (destroyed || generation !== apiKeyGeneration || controller.signal.aborted) return;
      const options = page.api_keys.map(apiKeyFilterOption);
      for (const option of options) {
        rememberedApiKeys = rememberOption(rememberedApiKeys, option);
      }
      apiKeyOptions = options;
    } catch (error) {
      if (destroyed || generation !== apiKeyGeneration || controller.signal.aborted) return;
      apiKeysError = localizedError(error, 'Could not search API keys.', tr);
      apiKeyOptions = [];
    } finally {
      if (!destroyed && generation === apiKeyGeneration) {
        apiKeysLoading = false;
        emit();
      }
    }
  }

  async function fetchProviders(): Promise<void> {
    providersError = '';
    emit();
    try {
      const providers = await api.providers();
      if (destroyed) return;
      const options = providers.map(providerFilterOption);
      for (const option of options) {
        rememberedProviders = rememberOption(rememberedProviders, option);
      }
      providerOptions = options;
    } catch (error) {
      if (destroyed) return;
      providersError = localizedError(error, 'Could not load providers.', tr);
      providerOptions = [];
    }
    if (!destroyed) emit();
  }

  return {
    searchApiKeys: (query: string) => {
      if (searchTimer !== null) clearTimeout(searchTimer);
      searchTimer = setTimeout(() => {
        searchTimer = null;
        void fetchApiKeys(query);
      }, API_KEY_SEARCH_DEBOUNCE_MS);
    },
    loadProviders: () => {
      void fetchProviders();
    },
    destroy: () => {
      destroyed = true;
      apiKeyGeneration += 1;
      cancelSearch();
    },
  };
}
