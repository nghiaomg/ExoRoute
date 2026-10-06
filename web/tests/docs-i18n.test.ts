import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import { docsCopy } from '../src/features/docs/docsCopy';
import type { Locale } from '../src/lib/i18n';

const locales: Locale[] = ['en', 'vi', 'zh', 'es', 'pt', 'ja', 'id', 'ko', 'de', 'fr', 'hi', 'ar'];

test('public documentation has a complete copy shape for every supported locale', () => {
  for (const locale of locales) {
    const copy = docsCopy(locale);

    assert.ok(copy.shell.documentation, `${locale} is missing the documentation label`);
    assert.ok(copy.home.title, `${locale} is missing the home title`);
    assert.equal(copy.home.cards.length, 3, `${locale} must keep all documentation cards`);
    assert.equal(copy.quickstart.steps.length, 4, `${locale} must keep all quickstart steps`);
    assert.equal(copy.integrations.guides.length, 7, `${locale} must keep all integration guides`);
    assert.equal(copy.reference.endpoints.length, 6, `${locale} must keep all API endpoints`);

    for (const guide of copy.integrations.guides) {
      assert.ok(guide.title, `${locale} has an integration without a title`);
      assert.ok(guide.summary, `${locale} has an integration without a summary`);
      assert.ok(guide.steps.length > 0, `${locale} has an integration without steps`);
    }
  }
});

test('every non-English locale localizes the public documentation shell and home page', () => {
  const english = docsCopy('en');

  for (const locale of locales.filter((value) => value !== 'en')) {
    const copy = docsCopy(locale);
    assert.ok(
      Object.values(copy.shell).some((value, index) => value !== Object.values(english.shell)[index]),
      `${locale} shell is still English`,
    );
    assert.notEqual(copy.home.title, english.home.title, `${locale} home page is still English`);
    assert.notEqual(copy.nav.docs.label, english.nav.docs.label, `${locale} navigation is still English`);
  }
});

test('localization keeps executable examples and technical contracts intact', () => {
  const english = docsCopy('en');

  for (const locale of locales.filter((value) => value !== 'en')) {
    const copy = docsCopy(locale);
    assert.equal(copy.quickstart.request.code, english.quickstart.request.code, `${locale} changed the curl contract`);
    assert.equal(copy.quickstart.powershell.code, english.quickstart.powershell.code, `${locale} changed the PowerShell contract`);
    assert.equal(copy.reference.authCode.code, english.reference.authCode.code, `${locale} changed the auth example`);
    assert.deepEqual(
      copy.integrations.connectionFields.slice(0, 3).map((field) => field.value),
      english.integrations.connectionFields.slice(0, 3).map((field) => field.value),
      `${locale} changed executable connection URLs`,
    );
  }
});
