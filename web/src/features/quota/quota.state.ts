import type { Provider, ProviderKey, ProviderUsageAccount } from '../../lib/types';

export type CredentialState = {
  keys: ProviderKey[];
  usageByKey: Record<string, ProviderUsageAccount>;
  nextCursor: string | null;
  pageCursors: Array<string | null>;
  loading: boolean;
  refreshing: boolean;
  error: string;
  generation: number;
};

export function supportsQuota(provider: Provider): boolean {
  return provider.capabilities?.usage_limits === true
    || provider.capabilities?.api_key_usage === true
    || provider.capabilities?.local_usage_meter === true;
}

export function supportsCredentialUsage(provider: Provider): boolean {
  return provider.capabilities?.usage_limits === true
    || provider.capabilities?.api_key_usage === true
    || provider.capabilities?.local_usage_meter === true;
}

export function createCredentialState(): CredentialState {
  return {
    keys: [],
    usageByKey: {},
    nextCursor: null,
    pageCursors: [null],
    loading: false,
    refreshing: false,
    error: '',
    generation: 0,
  };
}

export function credentialStateFor(
  providerId: string,
  states: Record<string, CredentialState>,
): CredentialState {
  return states[providerId] ?? createCredentialState();
}

export function hasCredentialItems(state: CredentialState | undefined): boolean {
  return (state?.keys.length ?? 0) > 0 || Object.keys(state?.usageByKey ?? {}).length > 0;
}

export function mergeProviderUsage(
  existingKeys: ProviderKey[],
  accounts: ProviderUsageAccount[],
  accountName: (id: string) => string,
  createdAt = new Date().toISOString(),
): { keys: ProviderKey[]; usageByKey: Record<string, ProviderUsageAccount> } {
  const existingKeyIds = new Set(existingKeys.map((key) => key.id));
  const syntheticKeys: ProviderKey[] = [];
  const usageByKey: Record<string, ProviderUsageAccount> = {};

  for (let index = 0; index < accounts.length; index += 1) {
    const account = accounts[index];
    const id = account.key_id || `account_${index}`;
    usageByKey[id] = account;
    if (existingKeyIds.has(id)) continue;
    existingKeyIds.add(id);
    syntheticKeys.push({
      id,
      name: account.name || accountName(id),
      credential_type: account.key_id ? 'api_key' : 'oauth',
      enabled: account.status !== 'disabled',
      invalid: account.status === 'reauth_required',
      created_at: createdAt,
    });
  }

  return { keys: [...existingKeys, ...syntheticKeys], usageByKey };
}
