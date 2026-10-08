import type { GatewayCombo } from '../../lib/types';

/**
 * The combos a combo may not nest: itself, and every combo that already reaches
 * it, directly or through other combos. Nesting one of those would close a
 * reference cycle, which the save path rejects; the editor never offers them.
 *
 * `loadedCombos` is the whole list, and the root's own outgoing references are
 * read from it: a combo's newly added references cannot make another combo
 * reach the root, so the current edit does not change the answer. The server
 * validates the graph again on save.
 */
export function cyclingComboIds(
  loadedCombos: GatewayCombo[],
  rootId: string | undefined,
): Set<string> {
  const blocking = new Set<string>();
  if (!rootId) return blocking;
  const parentsByChild = new Map<string, string[]>();
  for (const combo of loadedCombos) {
    for (const target of combo.targets) {
      if (!target.combo_id) continue;
      const parents = parentsByChild.get(target.combo_id);
      if (parents) {
        parents.push(combo.id);
      } else {
        parentsByChild.set(target.combo_id, [combo.id]);
      }
    }
  }
  const pending = [rootId];
  while (pending.length > 0) {
    const comboId = pending.pop();
    if (comboId === undefined || blocking.has(comboId)) continue;
    blocking.add(comboId);
    pending.push(...(parentsByChild.get(comboId) ?? []));
  }
  return blocking;
}

/** Mirrors `MAX_COMBO_NESTING_DEPTH` in src/config/limits.rs. */
const MAX_NESTING_DEPTH = 8;

/** Combos one expansion walks before it stops, mirroring the server's scan bound. */
const MAX_EXPANSION_NODES = 4_096;

interface Walk {
  byId: Map<string, GatewayCombo>;
  budget: number;
}

/**
 * Counts the provider targets a combo expands to, mirroring how the gateway
 * flattens nested references: a reference is replaced in place by the targets
 * of the combo it names, a repeated reference counts once per occurrence, and a
 * reference to a combo that is absent from `combos` contributes nothing.
 *
 * The walk is bounded rather than authoritative. A cyclic or over-deep graph is
 * rejected by the save path and again by the gateway, so this only has to stay
 * finite and produce a useful number for well-formed data.
 */
export function flattenedTargetCount(combo: GatewayCombo, combos: GatewayCombo[]): number {
  const walk: Walk = {
    byId: new Map(combos.map((entry) => [entry.id, entry])),
    budget: MAX_EXPANSION_NODES,
  };
  return countTargets(combo, walk, new Set<string>(), 0);
}

function countTargets(
  combo: GatewayCombo,
  walk: Walk,
  branch: ReadonlySet<string>,
  depth: number,
): number {
  // A combo already on this branch is a cycle; the save path rejects one, so
  // this branch simply contributes nothing instead of recursing forever.
  if (depth > MAX_NESTING_DEPTH || branch.has(combo.id) || walk.budget <= 0) return 0;
  walk.budget -= 1;
  const nextBranch = new Set(branch).add(combo.id);
  let total = 0;
  for (const target of combo.targets) {
    if (!target.combo_id) {
      total += 1;
      continue;
    }
    const nested = walk.byId.get(target.combo_id);
    if (nested) total += countTargets(nested, walk, nextBranch, depth + 1);
  }
  return total;
}
