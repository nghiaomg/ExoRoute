import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import { mergeImportedModels } from '../src/lib/provider-model-import';

const base = {
  available: true,
  authoritative: true,
  importedModels: [] as string[],
  previousModels: [] as string[],
  manualModels: [] as string[],
  pruned: [] as string[],
};

test('an authoritative import drops stale discovered rows and keeps manual models', () => {
  const merged = mergeImportedModels({
    ...base,
    importedModels: ['gpt-5.6-sol', 'claude-opus-5'],
    previousModels: ['gpt-5.6-sol', 'retired-model', 'operator-model'],
    manualModels: ['operator-model'],
  });

  assert.deepEqual(merged.models, ['claude-opus-5', 'gpt-5.6-sol', 'operator-model']);
  assert.deepEqual(merged.manualModels, ['operator-model']);
});

test('a manual model that upstream also reports stays flagged as manual', () => {
  const merged = mergeImportedModels({
    ...base,
    importedModels: ['operator-model', 'gpt-5.6-sol'],
    previousModels: ['operator-model'],
    manualModels: ['operator-model'],
  });

  assert.deepEqual(merged.models, ['gpt-5.6-sol', 'operator-model']);
  assert.deepEqual(merged.manualModels, ['operator-model']);
});

test('a non-authoritative catalog only adds rows', () => {
  const merged = mergeImportedModels({
    ...base,
    authoritative: false,
    importedModels: ['new-model'],
    previousModels: ['kept-model', 'operator-model'],
    manualModels: ['operator-model'],
  });

  assert.deepEqual(merged.models, ['kept-model', 'new-model', 'operator-model']);
  assert.deepEqual(merged.manualModels, ['operator-model']);
});

test('pruned rows are removed and a manual row survives pruning', () => {
  const merged = mergeImportedModels({
    ...base,
    importedModels: ['fresh-model'],
    previousModels: ['placeholder-model', 'operator-model', 'fresh-model'],
    manualModels: ['operator-model'],
    pruned: ['placeholder-model'],
  });

  assert.deepEqual(merged.models, ['fresh-model', 'operator-model']);
  assert.deepEqual(merged.manualModels, ['operator-model']);
});

test('an unavailable import leaves the catalog untouched', () => {
  const merged = mergeImportedModels({
    ...base,
    available: false,
    importedModels: [],
    previousModels: ['operator-model', 'kept-model'],
    manualModels: ['operator-model'],
  });

  assert.deepEqual(merged.models, ['operator-model', 'kept-model']);
  assert.deepEqual(merged.manualModels, ['operator-model']);
});

test('duplicate imported rows are reported once', () => {
  const merged = mergeImportedModels({ ...base, importedModels: ['dup', 'dup'] });

  assert.deepEqual(merged.models, ['dup']);
});
