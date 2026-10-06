import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import {
  MAX_CHAT_ATTACHMENTS,
  MAX_HISTORY_CHARS,
  MAX_HISTORY_TURNS,
  THINKING_MODES,
  branchFromEdit,
  buildChatHistory,
  contextBeforeFinalUserTurn,
  continueInstruction,
  conversationToJson,
  conversationToMarkdown,
  createDeltaBuffer,
  defaultModelId,
  dropTrailingAssistantTurns,
  emptyCompareColumn,
  emptyDraft,
  estimateSessionCost,
  findPromptByName,
  historyPressure,
  loadLastModelId,
  loadPriceTable,
  loadPrompts,
  makePrompt,
  modelDisplayName,
  modelProviderPrefix,
  parseGenerationOptions,
  parseSlashSave,
  parseSlashUse,
  raceWithDeadline,
  saveLastModelId,
  savePrompts,
  savePriceTable,
  sessionTokens,
  splitModelId,
  streamingAssistantMessage,
  totalAttachmentBytes,
  validateDraft,
} from '../src/features/chat/chat.state';
import {
  countCodeBlocks,
  escapeHtml,
  markdownPreview,
  renderMarkdown,
} from '../src/features/chat/markdown';
import type { ChatConversationMessage, WorkspaceChatModelOption } from '../src/lib/types';

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
    // A single-turn ask replays nothing and sends provider defaults.
    history: [],
    temperature: null,
    top_p: null,
    max_tokens: null,
  });

  // Model ids may themselves contain slashes; the first segment is the
  // prefix and the rest is the model.
  const nested = validateDraft({ ...draft, modelId: 'ocg/org/nested-model', message: 'hi' });
  assert.ok(nested.ok);
  assert.equal(nested.ok ? nested.input.model : '', 'org/nested-model');
});

test('validateDraft sends the saved provider id, falling back to the model alias prefix', () => {
  const draft = { ...emptyDraft(), modelId: 'ocg/minimax-m3', message: 'hi' };

  // The relay looks the provider up by its saved id, which the model list
  // reports separately from the `{prefix}/{model}` alias.
  const resolved = validateDraft({ ...draft, providerId: 'opencode-go' });
  assert.ok(resolved.ok);
  assert.equal(resolved.ok ? resolved.input.provider_id : '', 'opencode-go');
  assert.equal(resolved.ok ? resolved.input.model : '', 'minimax-m3');

  // A model the loaded list did not describe still falls back to its prefix.
  const fallback = validateDraft(draft);
  assert.ok(fallback.ok);
  assert.equal(fallback.ok ? fallback.input.provider_id : '', 'ocg');
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

test('history replay keeps the newest turns inside the relay budget', () => {
  const turns: ChatConversationMessage[] = [];
  for (let index = 0; index < MAX_HISTORY_TURNS + 6; index += 1) {
    turns.push({ id: `u${index}`, role: 'user', text: `question ${index}` });
    turns.push({ id: `a${index}`, role: 'assistant', text: `answer ${index}` });
  }
  const slice = buildChatHistory(turns);
  assert.equal(slice.turns, MAX_HISTORY_TURNS);
  assert.ok(slice.droppedTurns > 0);
  // Newest turns survive, oldest are the ones dropped, and order is oldest first.
  assert.equal(slice.history[slice.history.length - 1]?.text, 'answer 45');
  assert.equal(slice.history[0]?.text, 'question 26');
  assert.equal(historyPressure(slice), 'over');
});

test('history replay skips empty and failed turns and reports pressure', () => {
  const slice = buildChatHistory([
    { id: 'u1', role: 'user', text: '  ' },
    { id: 'a1', role: 'assistant', text: '', error: 'boom' },
    { id: 'u2', role: 'user', text: 'kept', attachmentCount: 2 },
  ]);
  assert.deepEqual(slice.history, [{ role: 'user', text: 'kept' }]);
  assert.equal(slice.turns, 1);
  assert.equal(slice.characters, 4);
  assert.equal(slice.droppedTurns, 0);
  assert.equal(historyPressure(slice), 'ok');

  const nearLimit = buildChatHistory([
    { id: 'a1', role: 'assistant', text: 'x'.repeat(Math.ceil(MAX_HISTORY_CHARS * 0.85)) },
  ]);
  assert.equal(historyPressure(nearLimit), 'near');
});

test('generation options parse within the relay ranges only', () => {
  assert.deepEqual(parseGenerationOptions(emptyDraft()), {
    ok: true,
    temperature: null,
    top_p: null,
    max_tokens: null,
  });
  assert.deepEqual(parseGenerationOptions({ ...emptyDraft(), temperature: ' 0.7 ', topP: '0.9', maxTokens: '512' }), {
    ok: true,
    temperature: 0.7,
    top_p: 0.9,
    max_tokens: 512,
  });
  const cases: Array<[string, string, string, string]> = [
    ['2.5', '', '', 'Temperature must be a number between 0 and 2.'],
    ['', '1.5', '', 'Top P must be a number between 0 and 1.'],
    ['', '', '0', 'Max tokens must be a whole number in the allowed range.'],
    ['', '', '12.5', 'Max tokens must be a whole number in the allowed range.'],
    ['warm', '', '', 'Temperature must be a number between 0 and 2.'],
  ];
  for (const [temperature, topP, maxTokens, errorKey] of cases) {
    const result = parseGenerationOptions({ ...emptyDraft(), temperature, topP, maxTokens });
    assert.equal(result.ok ? '' : result.errorKey, errorKey, `${temperature}/${topP}/${maxTokens}`);
  }
});

test('validateDraft carries the bounded history and options into the request', () => {
  const validated = validateDraft(
    { ...emptyDraft(), modelId: 'ocg/minimax-m3', message: 'next', temperature: '0.2', maxTokens: '64' },
    [
      { id: 'u1', role: 'user', text: 'first' },
      { id: 'a1', role: 'assistant', text: 'reply', model: 'minimax-m3', finish_reason: 'stop' },
    ],
  );
  assert.ok(validated.ok);
  const input = validated.ok ? validated.input : null;
  assert.deepEqual(input?.history, [
    { role: 'user', text: 'first' },
    { role: 'assistant', text: 'reply' },
  ]);
  assert.equal(input?.temperature, 0.2);
  assert.equal(input?.max_tokens, 64);
  assert.equal(input?.top_p, null);
});

test('session token totals add every reported usage', () => {
  const totals = sessionTokens([
    { id: 'a1', role: 'assistant', text: 'one', usage: { input_tokens: 10, output_tokens: 4 } },
    { id: 'u1', role: 'user', text: 'two' },
    { id: 'a2', role: 'assistant', text: 'three', usage: { input_tokens: 7, output_tokens: 3 } },
  ]);
  assert.deepEqual(totals, { input: 17, output: 7 });
});

/* ------------------------------------------------------------------ */
/* Edit, branch, regenerate, and continue                              */
/* ------------------------------------------------------------------ */

test('branchFromEdit rewrites the user turn and drops everything after it', () => {
  const conversation: ChatConversationMessage[] = [
    { id: 'u1', role: 'user', text: 'first' },
    { id: 'a1', role: 'assistant', text: 'reply', model: 'm' },
    { id: 'u2', role: 'user', text: 'second' },
  ];
  const branched = branchFromEdit(conversation, 2, 'second, but better');
  assert.notEqual(branched, conversation);
  assert.equal(branched.length, 3);
  assert.equal(branched[2].text, 'second, but better');
  assert.equal(branched[2].edited, true);
  assert.notEqual(branched[2].id, 'u2');

  // Non-user and out-of-range edits are rejected without mutation.
  assert.equal(branchFromEdit(conversation, 1, 'nope'), conversation);
  assert.equal(branchFromEdit(conversation, 9, 'nope'), conversation);
});

test('regenerate helpers slice off trailing assistant turns and context', () => {
  const conversation: ChatConversationMessage[] = [
    { id: 'u1', role: 'user', text: 'first' },
    { id: 'a1', role: 'assistant', text: 'reply one' },
    { id: 'u2', role: 'user', text: 'second' },
    { id: 'a2', role: 'assistant', text: 'reply two' },
    { id: 'e1', role: 'assistant', error: 'boom', text: '' },
  ];
  const kept = dropTrailingAssistantTurns(conversation);
  assert.deepEqual(kept.map((m) => m.id), ['u1', 'a1', 'u2']);

  // The page re-asks the last user turn as the message, so the history
  // slice is everything before it.
  const lastUser = kept[kept.length - 1];
  const context = contextBeforeFinalUserTurn([...kept, lastUser]);
  assert.deepEqual(context.map((m) => m.id), ['u1', 'a1']);
  assert.deepEqual(contextBeforeFinalUserTurn([{ id: 'u1', role: 'user', text: 'only' }]), []);

  const instruction = continueInstruction();
  assert.match(instruction, /Continue the previous reply/);
  assert.match(instruction, /Do not repeat/);
});

/* ------------------------------------------------------------------ */
/* Export: Markdown and JSON                                           */
/* ------------------------------------------------------------------ */

test('conversation exports round-trip markdown and JSON', () => {
  const conversation: ChatConversationMessage[] = [
    { id: 'u1', role: 'user', text: 'hello **world**' },
    { id: 'a1', role: 'assistant', text: 'hi', model: 'm3', reasoning: 'thought hard', usage: { input_tokens: 3, output_tokens: 1 } },
    { id: 'e1', role: 'assistant', error: 'upstream failed', text: '' },
  ];
  const md = conversationToMarkdown(conversation, 'My chat');
  assert.match(md, /^# My chat/);
  assert.match(md, /## You/);
  assert.match(md, /## m3/);
  assert.match(md, /<details><summary>Reasoning<\/summary>/);
  assert.match(md, /> \*\*Error:\*\* upstream failed/);

  const json = JSON.parse(conversationToJson(conversation)) as { exported_at: string; messages: ChatConversationMessage[] };
  assert.ok(json.exported_at);
  assert.equal(json.messages.length, 3);
  assert.equal(json.messages[1].usage?.output_tokens, 1);
});

/* ------------------------------------------------------------------ */
/* Markdown renderer: XSS-safe subset                                  */
/* ------------------------------------------------------------------ */

test('markdown renderer escapes hostile markup before any transformation', () => {
  const hostile = '<script>alert(1)<\/script> and [x](javascript:alert(2)) and <img src=x onerror=alert(3)>';
  const html = renderMarkdown(hostile);
  assert.ok(!html.includes('<script'));
  assert.ok(!html.includes('<img'));
  assert.ok(!html.includes('href="javascript:'));
  assert.ok(html.includes('&lt;script&gt;'));
  // The unsafe link scheme stays literal text.
  assert.ok(!html.includes('<a class="md-link" href="javascript:'));
});

test('markdown renderer supports the safe subset only', () => {
  const html = renderMarkdown(
    ['# Title', '', '- a', '- b', '', '```ts', 'const x = 1;', '```', '', 'plain **bold** and `code`'].join('\n'),
  );
  assert.match(html, /<h1 class="md-h md-h1">Title<\/h1>/);
  assert.match(html, /<ul class="md-list"><li>a<\/li><li>b<\/li><\/ul>/);
  assert.match(html, /<span class="md-code-lang">ts<\/span>/);
  assert.match(html, /<pre class="md-pre"><code>const x = 1;<\/code><\/pre>/);
  assert.match(html, /<strong class="md-strong">bold<\/strong>/);
  assert.match(html, /<code class="md-code">code<\/code>/);

  const linked = renderMarkdown('[docs](https://example.com) and [bad](ftp://example.com)');
  assert.match(linked, /href="https:\/\/example.com"/);
  assert.ok(!linked.includes('href="ftp:'));

  assert.equal(escapeHtml('a & < b "c" \'d\''), 'a &amp; &lt; b &quot;c&quot; &#39;d&#39;');
  assert.equal(countCodeBlocks('```a\nx``` \n ```b\ny```'), 2);
  assert.equal(markdownPreview('# Hi\n- one\n- two', 10), 'Hi - one…');
});

/* ------------------------------------------------------------------ */
/* Streaming buffers                                                   */
/* ------------------------------------------------------------------ */

test('delta buffer coalesces pushes into one apply per frame', async () => {
  const applies: Array<[string, string]> = [];
  const buffer = createDeltaBuffer((text, reasoning) => applies.push([text, reasoning]));
  buffer.push('He', '');
  buffer.push('llo', '');
  buffer.push('', ' thinking');
  // Nothing applied synchronously: one batch per frame.
  assert.equal(applies.length, 0);
  await new Promise((resolve) => setTimeout(resolve, 50));
  assert.equal(applies.length, 1);
  assert.deepEqual(applies[0], ['Hello', ' thinking']);

  buffer.flush();
  assert.equal(applies.length, 1); // empty pending state applies nothing
});

test('streaming assistant message and compare columns start empty', () => {
  const message = streamingAssistantMessage();
  assert.equal(message.role, 'assistant');
  assert.equal(message.text, '');
  assert.ok(message.id);

  const column = emptyCompareColumn('prov/model-x');
  assert.equal(column.modelId, 'prov/model-x');
  assert.equal(column.state, 'running');
  assert.equal(column.usage, null);
});

/* ------------------------------------------------------------------ */
/* Model identity and the picker default                               */
/* ------------------------------------------------------------------ */

test('model ids split into a routing prefix and a scannable model name', () => {
  const id = 'command-code/deepseek/deepseek-v4-flash-vision-exp';
  assert.deepEqual(splitModelId(id), {
    prefix: 'command-code',
    model: 'deepseek/deepseek-v4-flash-vision-exp',
  });
  // The picker shows the last segment, the part that tells siblings apart.
  assert.equal(modelDisplayName(id), 'deepseek-v4-flash-vision-exp');
  assert.equal(modelProviderPrefix(id), 'command-code');
  assert.equal(modelDisplayName('ocg/minimax-m3'), 'minimax-m3');

  // A bare name has no routing prefix and is shown unchanged.
  assert.equal(modelDisplayName('gpt-5'), 'gpt-5');
  assert.equal(modelProviderPrefix('gpt-5'), '');
  assert.equal(modelDisplayName(''), '');
});

test('the picker default keeps the current choice, then the remembered one, then the first option', () => {
  const options: WorkspaceChatModelOption[] = [
    { provider_id: 'p1', model: 'alpha', id: 'a/alpha' },
    { provider_id: 'p2', model: 'beta', id: 'b/beta' },
  ];
  assert.equal(defaultModelId(options, 'b/beta', 'a/alpha'), 'b/beta');
  assert.equal(defaultModelId(options, '', 'b/beta'), 'b/beta');
  // A stale current or remembered id must not survive a removed model.
  assert.equal(defaultModelId(options, 'gone/model', 'also/gone'), 'a/alpha');
  assert.equal(defaultModelId([], '', 'a/alpha'), '');
});

test('the remembered model survives a reload and clears with an empty choice', () => {
  withMemoryStorage(() => {
    assert.equal(loadLastModelId(), '');
    saveLastModelId('ocg/minimax-m3');
    assert.equal(loadLastModelId(), 'ocg/minimax-m3');
    assert.equal(globalThis.localStorage.getItem('exoroute.workspace-chat.model.v1'), 'ocg/minimax-m3');
    saveLastModelId('');
    assert.equal(loadLastModelId(), '');
  });
});

test('an unreadable remembered model is ignored', () => {
  withMemoryStorage(() => {
    (globalThis as Record<string, unknown>).localStorage = {
      getItem: () => 'x'.repeat(300),
      setItem: () => undefined,
      removeItem: () => undefined,
    } as unknown as MemoryStorage;
    assert.equal(loadLastModelId(), '');
  });
});

/* ------------------------------------------------------------------ */
/* Price table and prompt library (localStorage-backed)                */
/* ------------------------------------------------------------------ */

type MemoryStorage = { getItem: (key: string) => string | null; setItem: (key: string, value: string) => void; removeItem?: (key: string) => void };

function withMemoryStorage(run: () => void): void {
  const backing = new Map<string, string>();
  const storage: MemoryStorage = {
    getItem: (key) => backing.get(key) ?? null,
    setItem: (key, value) => void backing.set(key, value),
    removeItem: (key) => void backing.delete(key),
  };
  (globalThis as Record<string, unknown>).localStorage = storage;
  try {
    run();
  } finally {
    delete (globalThis as Record<string, unknown>).localStorage;
  }
}

test('price table estimates cost from operator-entered rows only', () => {
  withMemoryStorage(() => {
    assert.deepEqual(loadPriceTable(), {}); // no invented prices
    savePriceTable({
      'prov/m3': { currency: 'USD', inputPerMillion: 2, outputPerMillion: 8 },
    });
    const table = loadPriceTable();
    const cost = estimateSessionCost(table, 'prov/m3', 1_000_000, 500_000);
    assert.deepEqual(cost, { currency: 'USD', amount: 6 });
    assert.equal(estimateSessionCost(table, 'prov/unknown', 10, 10), null);
  });
});

test('price table load sanitizes hostile or corrupt entries', () => {
  withMemoryStorage(() => {
    (globalThis as Record<string, unknown>).localStorage = {
      getItem: () =>
        JSON.stringify({
          'ok/model': { currency: 'EUR', inputPerMillion: '3', outputPerMillion: -5 },
          '': { currency: 'USD', inputPerMillion: 1, outputPerMillion: 1 },
          bad: 'not-an-object',
        }),
      setItem: () => undefined,
    } as unknown as MemoryStorage;
    const table = loadPriceTable();
    assert.deepEqual(table['ok/model'], { currency: 'EUR', inputPerMillion: 3, outputPerMillion: 0 });
    assert.equal(table[''], undefined);
    assert.equal(table.bad, undefined);
  });

  withMemoryStorage(() => {
    (globalThis as Record<string, unknown>).localStorage = {
      getItem: () => 'not json at all',
      setItem: () => undefined,
    } as unknown as MemoryStorage;
    assert.deepEqual(loadPriceTable(), {});
  });
});

test('prompt library save/load and slash commands stay bounded and case-insensitive', () => {
  withMemoryStorage(() => {
    assert.deepEqual(loadPrompts(), []);
    const prompt = makePrompt('  Code Review  ', 'Review the diff carefully.');
    assert.equal(prompt.name, 'Code Review');
    assert.match(prompt.id, /^prompt-/);
    savePrompts([prompt]);
    assert.deepEqual(loadPrompts(), [prompt]);

    const parsed = parseSlashSave('/save keeper  do things  '); 
    assert.deepEqual(parsed, { name: 'keeper', body: 'do things' });
    assert.equal(parseSlashSave('just a message'), null);

    const used = parseSlashUse('/use code review', [prompt]);
    assert.equal(used?.id, prompt.id);
    assert.equal(parseSlashUse('/use missing', [prompt]), null);
    assert.equal(parseSlashUse('hello', [prompt]), null);

    assert.equal(findPromptByName([prompt], 'CODE REVIEW')?.id, prompt.id);
    assert.equal(findPromptByName([prompt], '  '), null);
  });
});

/* ------------------------------------------------------------------ */
/* Deadline-bounded model list loading                                 */
/* ------------------------------------------------------------------ */

test('raceWithDeadline returns the request value when it beats the deadline', async () => {
  const race = raceWithDeadline(async () => 'ok', 5_000);
  assert.deepEqual(await race.outcome, { kind: 'ok', value: 'ok' });
});

test('raceWithDeadline reports a failure as an outcome instead of rejecting', async () => {
  const failure = new Error('gateway down');
  const race = raceWithDeadline(() => Promise.reject(failure), 5_000);
  const outcome = await race.outcome;
  assert.equal(outcome.kind, 'error');
  assert.equal(outcome.kind === 'error' ? outcome.error : null, failure);
});

test('raceWithDeadline times out and aborts a request that never settles', async () => {
  let aborted = false;
  const race = raceWithDeadline(
    (signal) =>
      new Promise<never>((_resolve, reject) => {
        signal.addEventListener('abort', () => {
          aborted = true;
          reject(new Error('aborted'));
        });
      }),
    20,
  );
  assert.deepEqual(await race.outcome, { kind: 'timeout' });
  assert.equal(aborted, true);
});

test('a late request resolution cannot overwrite the timeout outcome', async () => {
  let finish: (value: string) => void = () => {};
  const race = raceWithDeadline(
    () => new Promise<string>((resolve) => { finish = resolve; }),
    20,
  );
  const outcome = await race.outcome;
  assert.deepEqual(outcome, { kind: 'timeout' });
  finish('late');
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.deepEqual(await outcome, { kind: 'timeout' });
});

test('cancel aborts the request and settles the race as cancelled', async () => {
  let aborted = false;
  const race = raceWithDeadline(
    (signal) =>
      new Promise<never>((_resolve, reject) => {
        signal.addEventListener('abort', () => {
          aborted = true;
          reject(new Error('aborted'));
        });
      }),
    5_000,
  );
  race.cancel();
  assert.deepEqual(await race.outcome, { kind: 'cancelled' });
  assert.equal(aborted, true);
});
