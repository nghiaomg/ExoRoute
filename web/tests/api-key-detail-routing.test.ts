import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { dashboardPages, isDocsPage, pageFromPath, pagePaths } from '../src/lib/navigation.ts';
describe('API key detail routing', () => {
  test('detail routes resolve to a parameterized dashboard page', () => {
    assert.equal(pageFromPath('/api-keys/key-123'), 'api-key-detail:key-123');
    assert.equal(pageFromPath('/api-keys/key-123/'), 'api-key-detail:key-123');
    assert.equal(pageFromPath('/api-keys'), 'api-keys');
  });
  test('detail is a dashboard route and not a docs route', () => {
    assert.ok(dashboardPages.includes('api-key-detail'));
    assert.equal(isDocsPage('api-key-detail'), false);
  });
});
