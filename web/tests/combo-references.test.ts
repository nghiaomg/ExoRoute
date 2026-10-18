import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import { cyclingComboIds, flattenedTargetCount } from '../src/features/combos/references';
import type { ComboTarget, GatewayCombo } from '../src/lib/types';

function providerTarget(model: string): ComboTarget {
  return { provider_id: 'openai', model, priority: 1, enabled: true };
}

function nestedTarget(comboId: string): ComboTarget {
  return { provider_id: '', model: '', combo_id: comboId, priority: 1, enabled: true };
}

function combo(id: string, targets: ComboTarget[]): GatewayCombo {
  return { id, name: id, strategy: 'priority', accepted_protocols: ['chat_completions'], targets };
}

test('flattenedTargetCount substitutes a nested combo in place', () => {
  const child = combo('child', [providerTarget('a'), providerTarget('b')]);
  const parent = combo('parent', [providerTarget('x'), nestedTarget('child')]);

  assert.equal(flattenedTargetCount(parent, [child, parent]), 3);
});

test('flattenedTargetCount counts a repeated reference once per occurrence', () => {
  const child = combo('child', [providerTarget('a'), providerTarget('b')]);
  const parent = combo('parent', [nestedTarget('child'), nestedTarget('child')]);

  assert.equal(flattenedTargetCount(parent, [child, parent]), 4);
});

test('flattenedTargetCount ignores a reference to a combo that is not loaded', () => {
  const parent = combo('parent', [providerTarget('x'), nestedTarget('gone')]);

  assert.equal(flattenedTargetCount(parent, [parent]), 1);
});

test('flattenedTargetCount terminates on a reference cycle', () => {
  const first = combo('first', [nestedTarget('second')]);
  const second = combo('second', [nestedTarget('first')]);

  assert.equal(flattenedTargetCount(first, [first, second]), 0);
});

test('flattenedTargetCount stops below the supported nesting depth', () => {
  // Nine combos in a chain: the ninth sits at depth 8, the deepest supported,
  // and the tenth would sit past it.
  const chain = Array.from({ length: 10 }, (_, index) =>
    combo(`c${index}`, index === 9 ? [providerTarget('leaf')] : [nestedTarget(`c${index + 1}`)]));

  assert.equal(flattenedTargetCount(chain[0], chain), 0);
  assert.equal(flattenedTargetCount(chain[1], chain), 1);
});

test('cyclingComboIds blocks the combo and everything that reaches it', () => {
  const a = combo('a', [nestedTarget('b')]);
  const b = combo('b', [nestedTarget('c')]);
  const c = combo('c', [providerTarget('leaf')]);
  const sibling = combo('sibling', [providerTarget('leaf')]);

  assert.deepEqual([...cyclingComboIds([a, b, c, sibling], 'c')].sort(), ['a', 'b', 'c']);
  assert.deepEqual([...cyclingComboIds([a, b, c, sibling], 'a')], ['a']);
});

test('cyclingComboIds leaves a combo that cannot reach the root selectable', () => {
  // Editing `b`: `c` is a child of `b` and stays selectable (nesting it again
  // is a duplicate, not a cycle), while the parent `a` is blocked.
  const a = combo('a', [nestedTarget('b')]);
  const b = combo('b', [nestedTarget('c')]);
  const c = combo('c', [providerTarget('leaf')]);

  assert.deepEqual([...cyclingComboIds([a, b, c], 'b')].sort(), ['a', 'b']);
});

test('cyclingComboIds blocks nothing while creating a combo', () => {
  const existing = combo('existing', [nestedTarget('other')]);

  assert.equal(cyclingComboIds([existing], undefined).size, 0);
});
