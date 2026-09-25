import { api } from '../../lib/api';
import { localizedError } from '../../lib/errors';
import type { Translate } from '../../lib/format';
import type { Provider } from '../../lib/types';
import {
  credentialStateFor,
  createCredentialState,
  hasCredentialItems,
  mergeProviderUsage,
  supportsCredentialUsage,
  supportsQuota,
  type CredentialState,
} from './quota.state';

export type QuotaCredentialsController = {
  /** Load the provider list and the first credential page of each provider. */
  load: () => Promise<void>;
  /** Reload only providers that already have credential items. */
  refreshAll: () => Promise<void>;
  /** Force-refresh credentials for a single provider. */
  refreshProvider: (providerId: string) => Promise<void>;
  /** Advance to the next credential page for a provider. */
  nextPage: (providerId: string) => Promise<void>;
  /** Return to the previous credential page for a provider. */
  previousPage: (providerId: string) => Promise<void>;
  /** Reload the first page of a provider (used after budget edits). */
  reloadProvider: (providerId: string) => Promise<void>;
  /** Stop in-flight work from mutating state after component teardown. */
  destroy: () => void;
};

type QuotaCredentialsOptions = {
  /** Mirror of the reactive component state; read at call time. */
  getProviders: () => Provider[];
  getCredentialStates: () => Record<string, CredentialState>;
  onProviders: (providers: Provider[]) => void;
  onCredentialStates: (states: Record<string, CredentialState>) => void;
  onProvidersLoading: (loading: boolean) => void;
  onProviderError: (message: string) => void;
  onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  onProviderCountChange: (count: number) => void;
  tr: Translate;
};

const PROVIDER_REQUEST_CONCURRENCY = 4;

/**
 * Owns quota page data flow: provider listing, credential pagination,
 * concurrent batched loading, refresh coordination, and stale-generation
 * guards. The component stays presentational and reactive.
 */
export function createQuotaCredentialsController(options: QuotaCredentialsOptions): QuotaCredentialsController {
  let providerGeneration = 0;
  let lifecycleGeneration = 0;
  let providers: Provider[] = options.getProviders();
  let credentialStates: Record<string, CredentialState> = options.getCredentialStates();

  function updateCredentialState(providerId: string, patch: Partial<CredentialState>): void {
    credentialStates = {
      ...credentialStates,
      [providerId]: { ...credentialStateFor(providerId, credentialStates), ...patch },
    };
    options.onCredentialStates(credentialStates);
  }

  function snapshotProviders(result: Provider[]): Provider[] {
    providers = result;
    options.onProviders(providers);
    options.onProviderCountChange(result.length);
    const supported = result.filter(supportsQuota);
    const nextCredentialStates: Record<string, CredentialState> = {};
    for (const provider of supported) {
      nextCredentialStates[provider.id] = credentialStates[provider.id] ?? createCredentialState();
    }
    credentialStates = nextCredentialStates;
    options.onCredentialStates(credentialStates);
    return supported;
  }

  async function loadProviders(): Promise<void> {
    const generation = ++providerGeneration;
    const lifecycle = lifecycleGeneration;
    options.onProvidersLoading(true);
    options.onProviderError('');
    options.onConnectionChange('loading');
    try {
      const result = await api.providers();
      if (generation !== providerGeneration || lifecycle !== lifecycleGeneration) return;
      const supported = snapshotProviders(result);
      await loadCredentialPages(supported, generation, lifecycle);
      if (generation !== providerGeneration || lifecycle !== lifecycleGeneration) return;
      options.onConnectionChange('loaded');
    } catch (error) {
      if (generation !== providerGeneration || lifecycle !== lifecycleGeneration) return;
      options.onProviderError(localizedError(error, 'Something went wrong while loading this page.', options.tr));
      options.onConnectionChange('error');
    } finally {
      if (generation === providerGeneration && lifecycle === lifecycleGeneration) options.onProvidersLoading(false);
    }
  }

  async function loadCredentialPages(
    supported: Provider[],
    providerGenerationToken: number,
    lifecycleToken: number,
  ): Promise<void> {
    const providersWithUsage = supported.filter(supportsCredentialUsage);
    for (let index = 0; index < providersWithUsage.length; index += PROVIDER_REQUEST_CONCURRENCY) {
      if (providerGenerationToken !== providerGeneration || lifecycleToken !== lifecycleGeneration) return;
      const batch = providersWithUsage.slice(index, index + PROVIDER_REQUEST_CONCURRENCY);
      await Promise.all(batch.map((provider) => (
        loadCredentialPage(provider.id, providerGenerationToken, lifecycleToken)
      )));
    }
  }

  function accountDisplayName(id: string): string {
    return options.tr('Account ({id})', { id: id.slice(0, 8) });
  }

  async function loadCredentialPage(
    providerId: string,
    providerGenerationToken = providerGeneration,
    lifecycleToken = lifecycleGeneration,
  ): Promise<void> {
    const provider = providers.find((candidate) => candidate.id === providerId && supportsQuota(candidate));
    if (!provider || !supportsCredentialUsage(provider)) return;

    const current = credentialStateFor(providerId, credentialStates);
    const generation = current.generation + 1;
    const cursor = current.pageCursors[current.pageCursors.length - 1] ?? undefined;
    updateCredentialState(providerId, {
      loading: true,
      refreshing: false,
      error: '',
      generation,
    });
    try {
      const [keyPageRes, usagePageRes] = await Promise.allSettled([
        api.providerKeys(providerId, cursor),
        api.providerUsage(providerId, cursor),
      ]);

      if (
        providerGenerationToken !== providerGeneration
        || lifecycleToken !== lifecycleGeneration
        || credentialStates[providerId]?.generation !== generation
      ) return;

      if (keyPageRes.status === 'rejected' && usagePageRes.status === 'rejected') {
        const primaryError = keyPageRes.reason || usagePageRes.reason;
        updateCredentialState(providerId, {
          error: localizedError(primaryError, 'Could not load usage limits.', options.tr),
          loading: false,
        });
        return;
      }

      const keyPage = keyPageRes.status === 'fulfilled' ? keyPageRes.value : { keys: [], next_cursor: null };
      const usagePage = usagePageRes.status === 'fulfilled' ? usagePageRes.value : { accounts: [], next_cursor: null };

      const merged = mergeProviderUsage(
        keyPage.keys ?? [],
        usagePage.accounts ?? [],
        accountDisplayName,
      );

      updateCredentialState(providerId, {
        keys: merged.keys,
        usageByKey: merged.usageByKey,
        nextCursor: keyPage.next_cursor ?? usagePage.next_cursor ?? null,
        loading: false,
        error: '',
      });
    } catch (error) {
      if (
        providerGenerationToken === providerGeneration
        && lifecycleToken === lifecycleGeneration
        && credentialStates[providerId]?.generation === generation
      ) {
        updateCredentialState(providerId, {
          error: localizedError(error, 'Could not load usage limits.', options.tr),
          loading: false,
        });
      }
    } finally {
      // Unconditionally reset loading to ensure UI never hangs on loading spinner
      if (lifecycleToken === lifecycleGeneration) {
        updateCredentialState(providerId, { loading: false });
      }
    }
  }

  async function refreshCredentials(): Promise<void> {
    const targets = providers
      .filter(supportsCredentialUsage)
      .filter((provider) => {
        const state = credentialStates[provider.id];
        return hasCredentialItems(state) && !state?.loading && !state?.refreshing;
      });
    for (let index = 0; index < targets.length; index += PROVIDER_REQUEST_CONCURRENCY) {
      const batch = targets.slice(index, index + PROVIDER_REQUEST_CONCURRENCY);
      await Promise.all(batch.map((provider) => refreshProviderCredentials(provider.id)));
    }
  }

  async function refreshProviderCredentials(providerId: string): Promise<void> {
    const provider = providers.find((candidate) => candidate.id === providerId && supportsQuota(candidate));
    const current = credentialStateFor(providerId, credentialStates);
    if (!provider || !supportsCredentialUsage(provider) || !hasCredentialItems(current) || current.loading || current.refreshing) return;

    const providerGenerationToken = providerGeneration;
    const lifecycleToken = lifecycleGeneration;
    const generation = current.generation + 1;
    const cursor = current.pageCursors[current.pageCursors.length - 1] ?? undefined;
    updateCredentialState(providerId, { refreshing: true, error: '', generation });
    try {
      const result = await api.refreshProviderUsage(providerId, cursor);
      if (
        providerGenerationToken !== providerGeneration
        || lifecycleToken !== lifecycleGeneration
        || credentialStates[providerId]?.generation !== generation
      ) return;

      const merged = mergeProviderUsage(
        current.keys,
        result.accounts ?? [],
        accountDisplayName,
      );

      updateCredentialState(providerId, {
        keys: merged.keys,
        usageByKey: merged.usageByKey,
        nextCursor: result.next_cursor ?? current.nextCursor,
        refreshing: false,
      });
    } catch (error) {
      if (
        providerGenerationToken === providerGeneration
        && lifecycleToken === lifecycleGeneration
        && credentialStates[providerId]?.generation === generation
      ) {
        updateCredentialState(providerId, {
          error: localizedError(error, 'Could not load usage limits.', options.tr),
          refreshing: false,
        });
      }
    } finally {
      if (lifecycleToken === lifecycleGeneration) {
        updateCredentialState(providerId, { refreshing: false });
      }
    }
  }

  async function nextPage(providerId: string): Promise<void> {
    const state = credentialStateFor(providerId, credentialStates);
    if (!state.nextCursor || state.loading || state.refreshing) return;
    updateCredentialState(providerId, { pageCursors: [...state.pageCursors, state.nextCursor] });
    await loadCredentialPage(providerId);
  }

  async function previousPage(providerId: string): Promise<void> {
    const state = credentialStateFor(providerId, credentialStates);
    if (state.pageCursors.length <= 1 || state.loading || state.refreshing) return;
    updateCredentialState(providerId, { pageCursors: state.pageCursors.slice(0, -1) });
    await loadCredentialPage(providerId);
  }

  return {
    load: loadProviders,
    refreshAll: refreshCredentials,
    refreshProvider: refreshProviderCredentials,
    nextPage,
    previousPage,
    reloadProvider: (providerId: string) => loadCredentialPage(providerId),
    destroy: () => {
      providerGeneration += 1;
      lifecycleGeneration += 1;
    },
  };
}
