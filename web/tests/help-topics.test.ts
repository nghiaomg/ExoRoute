import { strict as assert } from 'node:assert';
import test from 'node:test';
import type { Translate } from '../src/lib/format';
import { errorCodes, faqTopics, filterHelpItems } from '../src/features/help/helpTopics';

const translate: Translate = (key) => key;

test('Help topic search matches normalized question text and hidden keywords', () => {
  const topics = faqTopics(translate);
  const codes = errorCodes(translate);

  assert.equal(filterHelpItems(topics, '  BACKUP ', (topic) => `${topic.title} ${topic.body} ${topic.keywords}`).length, 1);
  assert.equal(filterHelpItems(topics, ' ROTATION ', (topic) => `${topic.title} ${topic.body} ${topic.keywords}`).length, 1);
  assert.deepEqual(filterHelpItems(topics, '   ', (topic) => topic.title), topics);
  assert.equal(filterHelpItems(codes, '429', (code) => `${code.status} ${code.meaning} ${code.detail} ${code.keywords}`).length, 1);
});

test('Help error-code reference stays within the advertised gateway contract', () => {
  const codes = errorCodes(translate);

  assert.deepEqual(codes.map((code) => code.status), [401, 403, 404, 429, 502, 503, 504]);
  assert.ok(codes.every((code) => code.reason.length > 0 && code.detail.length > 0));
});
