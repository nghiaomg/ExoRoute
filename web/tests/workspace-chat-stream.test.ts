import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import { ApiError, api, setAdminAccessToken } from '../src/lib/api';
import type { WorkspaceChatInput } from '../src/lib/types';

/** One minimal wire input: the transport tests exercise framing, not drafts. */
const input: WorkspaceChatInput = {
  provider_id: 'provider',
  model: 'sample-model',
  message: 'hi',
  system_prompt: null,
  thinking_mode: 'default',
  thinking_override: null,
  attachments: [],
  history: [],
  temperature: null,
  top_p: null,
  max_tokens: null,
};

type Handlers = Parameters<typeof api.chatStream>[1];

type Captured = { deltas: string[]; reasoning: string[]; errors: string[]; done: unknown };

function captureHandlers(captured: Captured): Handlers {
  return {
    onTextDelta: (delta) => captured.deltas.push(delta),
    onReasoningDelta: (delta) => captured.reasoning.push(delta),
    onDone: (done) => {
      captured.done = done;
    },
    onError: (message) => captured.errors.push(message),
  };
}

function emptyCapture(): Captured {
  return { deltas: [], reasoning: [], errors: [], done: null };
}

async function withFetch<T>(handler: (url: string, init?: RequestInit) => Promise<Response>, run: () => Promise<T>): Promise<T> {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = ((url: string, init?: RequestInit) => handler(String(url), init)) as typeof fetch;
  try {
    return await run();
  } finally {
    globalThis.fetch = originalFetch;
    setAdminAccessToken('', 0);
  }
}

test('the chat stream authenticates and turns canonical SSE frames into deltas', async () => {
  const payload = [
    ': keep-alive\n\n',
    'event: reasoning-delta\ndata: {"delta":"think"}\n\n',
    'event: text-delta\ndata: {"delta":"Hel"}\n\n',
    'event: text-delta\ndata: {"delta":"lo"}\n\n',
    'event: done\ndata: {"model":"sample-model","finish_reason":"stop","input_tokens":5,"output_tokens":2,"duration_ms":12}\n\n',
  ].join('');
  const calls: { url: string; method: string | undefined; auth: string | null; body: unknown }[] = [];
  setAdminAccessToken('admin-token', 600);
  const captured = emptyCapture();

  await withFetch(
    async (url, init) => {
      calls.push({
        url,
        method: init?.method,
        auth: new Headers(init?.headers).get('Authorization'),
        body: typeof init?.body === 'string' ? JSON.parse(init.body) : null,
      });
      return new Response(payload, {
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
      });
    },
    () => api.chatStream(input, captureHandlers(captured)),
  );

  assert.equal(calls.length, 1);
  assert.equal(calls[0]?.url, '/api/v1/admin/workspace/chat/stream');
  assert.equal(calls[0]?.method, 'POST');
  assert.equal(calls[0]?.auth, 'Bearer admin-token');
  assert.deepEqual(calls[0]?.body, input);
  assert.deepEqual(captured.deltas, ['Hel', 'lo']);
  assert.deepEqual(captured.reasoning, ['think']);
  assert.deepEqual(captured.errors, []);
  assert.deepEqual(captured.done, {
    model: 'sample-model',
    finish_reason: 'stop',
    input_tokens: 5,
    output_tokens: 2,
    duration_ms: 12,
  });
});

test('an in-stream error event reaches the page as a terminal error, not a rejection', async () => {
  const payload = [
    'event: text-delta\ndata: {"delta":"partial"}\n\n',
    'event: error\ndata: {"message":"provider stopped mid-stream"}\n\n',
  ].join('');
  const captured = emptyCapture();

  await withFetch(
    async () =>
      new Response(payload, { status: 200, headers: { 'content-type': 'text/event-stream' } }),
    () => api.chatStream(input, captureHandlers(captured)),
  );

  assert.deepEqual(captured.deltas, ['partial']);
  assert.deepEqual(captured.errors, ['provider stopped mid-stream']);
  assert.equal(captured.done, null);
});

test('a rejected chat stream keeps the relay message so the page can show the cause', async () => {
  const detail = "provider 'opencode-go' returned HTTP 404: model not found";
  const captured = emptyCapture();

  await withFetch(
    async () =>
      new Response(JSON.stringify({ error: { code: '', message: detail } }), {
        status: 502,
        headers: { 'content-type': 'application/json' },
      }),
    async () => {
      await assert.rejects(
        () => api.chatStream(input, captureHandlers(captured)),
        (error: unknown) => {
          assert.ok(error instanceof ApiError);
          assert.equal(error.message, detail);
          assert.equal(error.status, 502);
          return true;
        },
      );
    },
  );

  assert.deepEqual(captured.errors, []);
});

test('a chat stream without a body reports a localized transport failure', async () => {
  await withFetch(
    async () => new Response(null, { status: 200 }),
    async () => {
      await assert.rejects(
        () => api.chatStream(input, captureHandlers(emptyCapture())),
        (error: unknown) => error instanceof ApiError && error.message.length > 0,
      );
    },
  );
});
