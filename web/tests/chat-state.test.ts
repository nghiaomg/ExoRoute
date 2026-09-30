import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import {
  MAX_CHAT_ATTACHMENTS,
  THINKING_MODES,
  emptyDraft,
  totalAttachmentBytes,
  validateDraft,
} from '../src/features/chat/chat.state';

test('validateDraft requires a selected model and a non-empty message', () => {
  const draft = emptyDraft();
  const noModel = validateDraft(draft);
  assert.equal(noModel.ok, false);
  assert.equal(noModel.ok ? '' : noModel.errorKey, 'Select a model before sending.');

  const noMessage = validateDraft({ ...draft, modelId: 'ocg/minimax-m3' });
  assert.equal(noMessage.ok ? '' : noMessage.errorKey, 'Type a message before sending.');

  const valid = validateDraft({
    ...draft,
    modelId: 'ocg/minimax-m3',
    message: '  hello  ',
    systemPrompt: '  be brief  ',
  });
  assert.ok(valid.ok);
  assert.deepEqual(valid.ok ? valid.input : null, {
    provider_id: 'ocg',
    model: 'minimax-m3',
    message: 'hello',
    system_prompt: 'be brief',
    thinking_mode: 'default',
    thinking_override: null,
    attachments: [],
  });

  // Model ids may themselves contain slashes; the first segment is the
  // prefix and the rest is the model.
  const nested = validateDraft({ ...draft, modelId: 'ocg/org/nested-model', message: 'hi' });
  assert.ok(nested.ok);
  assert.equal(nested.ok ? nested.input.model : '', 'org/nested-model');
});

test('validateDraft rejects override mode without replacement text and trims nothing of the payload', () => {
  const draft = { ...emptyDraft(), modelId: 'a/b', message: 'hi', thinkingMode: 'override' as const };
  const missing = validateDraft(draft);
  assert.equal(missing.ok ? '' : missing.errorKey, 'Thinking override text is required.');

  const ok = validateDraft({ ...draft, thinkingOverride: ' think step by step ' });
  assert.ok(ok);
  assert.equal(ok.ok ? ok.input.thinking_override : '', ' think step by step ');
});

test('attachment size accounting decodes base64 length and enforces ceilings', () => {
  assert.equal(totalAttachmentBytes([{ kind: 'image', media_type: 'image/png', data: 'aGVsbG8=' }]), 5);
  // Padded and unpadded encodings of the same bytes measure identically.
  assert.equal(
    totalAttachmentBytes([{ kind: 'image', media_type: 'image/png', data: 'aGVsbG8' }]),
    5,
  );
  const draft = {
    ...emptyDraft(),
    modelId: 'a/b',
    message: 'hi',
    attachments: Array.from({ length: MAX_CHAT_ATTACHMENTS + 1 }, () => ({
      kind: 'image' as const,
      media_type: 'image/png',
      data: 'aGVsbG8=',
    })),
  };
  const rejected = validateDraft(draft);
  assert.equal(rejected.ok ? '' : rejected.errorKey, 'Too many attachments.');
});

test('thinking modes expose the provider default first', () => {
  assert.equal(THINKING_MODES[0]?.value, 'default');
  assert.equal(THINKING_MODES.length, 4);
});
