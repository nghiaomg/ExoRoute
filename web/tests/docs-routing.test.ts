import { describe, expect, test } from 'node:test';
import assert from 'node:assert/strict';
import { docsPages, isDocsPage, pageFromPath, pagePaths } from '../src/lib/navigation';
describe('documentation workspace routing', () => {
  test('all docs routes map to nested /docs/* URLs', () => {
    assert.equal(pagePaths.docs, '/docs');
    assert.equal(pagePaths['docs-overview'], '/docs/overview');
    assert.equal(pagePaths['docs-quickstart'], '/docs/quickstart');
    assert.equal(pagePaths['docs-integrations'], '/docs/integrations');
    assert.equal(pagePaths['docs-reference'], '/docs/reference');
  });
  test('pageFromPath resolves nested docs and the api-reference alias', () => {
    assert.equal(pageFromPath('/docs'), 'docs');
    assert.equal(pageFromPath('/docs/overview'), 'docs-overview');
    assert.equal(pageFromPath('/docs/quickstart'), 'docs-quickstart');
    assert.equal(pageFromPath('/docs/integrations'), 'docs-integrations');
    assert.equal(pageFromPath('/docs/reference'), 'docs-reference');
    assert.equal(pageFromPath('/docs/api-reference'), 'docs-reference');
    assert.equal(pageFromPath('/docs/reference/'), 'docs-reference');
  });
  test('docs pages are classified as docs, not dashboard pages', () => {
    for (const page of docsPages) { assert.equal(isDocsPage(page), true); }
    assert.equal(isDocsPage('overview'), false);
  });
});
