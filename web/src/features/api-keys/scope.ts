import type { Provider } from '../../lib/types';

/** Options an add-picker renders before the operator narrows the search. */
export const MAX_VISIBLE_SCOPE_PROVIDERS = 30;

/** Splits the model-rule editor into trimmed, non-empty rules. */
export function parseModelRules(text: string): string[] {
  return text
    .split('\n')
    .map((rule) => rule.trim())
    .filter((rule) => rule.length > 0);
}

/** Renders stored rules back into the editor, one rule per line. */
export function modelRulesText(rules: string[] | null | undefined): string {
  return (rules ?? []).join('\n');
}

export interface AddableScopeProviders {
  options: Provider[];
  truncated: boolean;
}

/**
 * Providers the scope picker may still add: those the key does not allow yet,
 * narrowed by a search term and bounded to keep the dropdown small.
 */
export function addableScopeProviders(
  providers: Provider[],
  selectedProviderIds: string[],
  query: string,
): AddableScopeProviders {
  const normalized = query.trim().toLocaleLowerCase();
  const selected = new Set(selectedProviderIds);
  const matching = providers.filter(
    (provider) =>
      !selected.has(provider.id) &&
      (!normalized ||
        provider.name.toLocaleLowerCase().includes(normalized) ||
        provider.id.toLocaleLowerCase().includes(normalized)),
  );
  return {
    options: matching.slice(0, MAX_VISIBLE_SCOPE_PROVIDERS),
    truncated: matching.length > MAX_VISIBLE_SCOPE_PROVIDERS,
  };
}

/** Adds or removes one provider from the scope without creating duplicates. */
export function toggleProviderScope(
  selected: string[],
  providerId: string,
  allowed: boolean,
): string[] {
  const next = allowed
    ? [...selected, providerId]
    : selected.filter((id) => id !== providerId);
  return next.filter((id, index) => next.indexOf(id) === index);
}
