/**
 * Merge a provider model import response into the catalog the dashboard shows.
 *
 * The displayed list must mirror what the gateway stored: an authoritative
 * import replaces the discovered rows, but rows the operator added by hand
 * survive it, so they stay listed and keep their manual badge. A
 * non-authoritative catalog only adds rows, and a failed import changes
 * nothing.
 */
export interface ProviderModelImportMerge {
  available: boolean;
  /** The adapter owns its discovered rows, so absent ones disappear. */
  authoritative: boolean;
  importedModels: readonly string[];
  previousModels: readonly string[];
  manualModels: readonly string[];
  pruned: readonly string[];
}

export interface MergedProviderModels {
  models: string[];
  manualModels: string[];
}

export function mergeImportedModels(input: ProviderModelImportMerge): MergedProviderModels {
  const imported = unique(input.importedModels);
  let models: string[];
  if (!input.available) {
    models = [...input.previousModels];
  } else if (input.pruned.length > 0) {
    const pruned = new Set(input.pruned);
    models = unique([
      ...input.previousModels.filter((model) => !pruned.has(model)),
      ...imported,
    ]);
  } else if (!input.authoritative) {
    models = unique([...input.previousModels, ...imported]);
  } else {
    const discovered = new Set(imported);
    // Manual rows the provider no longer reports are still stored, so keep them
    // listed instead of hiding them until the next reload.
    models = unique([
      ...imported,
      ...input.manualModels.filter((model) => !discovered.has(model)),
    ]);
  }
  if (input.available) {
    models.sort((left, right) => left.localeCompare(right));
  }
  const listed = new Set(models);
  return { models, manualModels: input.manualModels.filter((model) => listed.has(model)) };
}

function unique(models: readonly string[]): string[] {
  return Array.from(new Set(models));
}
