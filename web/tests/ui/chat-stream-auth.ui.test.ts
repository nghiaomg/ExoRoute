import { afterEach, describe, expect, test, vi } from 'vitest';
import { ApiError, api, setAdminAccessToken } from '../../src/lib/api';
import type { WorkspaceChatInput } from '../../src/lib/types';

/**
 * The chat stream is the one transport the dashboard drives with its own
 * fetch loop, so these tests pin the admin-auth contract it must keep: refresh
 * a token that expired while the page was open, and hand a dead session back to
 * sign-in with the session message instead of a generic chat failure.
 */

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

afterEach(() => {
  vi.restoreAllMocks();
  setAdminAccessToken('', 0);
  try {
    localStorage.clear();
  } catch {
    // Storage may be unavailable; the auth coordination tolerates it.
  }
});

describe('workspace chat stream authentication', () => {
  test('refreshes a token that expired while the dashboard was open, then starts the turn', async () => {
    const calls: { url: string; auth: string | null }[] = [];
    vi.spyOn(globalThis, 'fetch').mockImplementation(async (target, init) => {
      calls.push({
        url: String(target),
        auth: new Headers(init?.headers).get('Authorization'),
      });
      if (String(target).endsWith('/auth/refresh')) {
        return new Response(
          JSON.stringify({
            authenticated: true,
            access_token: 'fresh-token',
            access_expires_in_seconds: 600,
            must_change_password: false,
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        );
      }
      return new Response(
        'event: done\ndata: {"model":"sample-model","finish_reason":"stop","input_tokens":1,"output_tokens":2,"duration_ms":3}\n\n',
        { status: 200, headers: { 'content-type': 'text/event-stream' } },
      );
    });
    setAdminAccessToken('stale-token', 1);
    let done: unknown = null;

    await api.chatStream(input, {
      onTextDelta: () => {},
      onDone: (payload) => {
        done = payload;
      },
      onError: () => {},
    });

    expect(calls.map((call) => call.url)).toEqual([
      '/api/v1/admin/auth/refresh',
      '/api/v1/admin/workspace/chat/stream',
    ]);
    expect(calls[1]?.auth).toBe('Bearer fresh-token');
    expect(done).toMatchObject({ model: 'sample-model', input_tokens: 1, output_tokens: 2 });
  });

  test('returns a dead admin session to sign-in with the session message', async () => {
    const authEvents: string[] = [];
    window.addEventListener('exoroute:auth-required', () => authEvents.push('required'));
    const calls: string[] = [];
    vi.spyOn(globalThis, 'fetch').mockImplementation(async (target) => {
      calls.push(String(target));
      if (String(target).endsWith('/auth/refresh')) {
        return new Response(
          JSON.stringify({
            error: { code: 'admin_refresh_invalid', message: 'refresh token is invalid' },
          }),
          { status: 401, headers: { 'content-type': 'application/json' } },
        );
      }
      return new Response(
        JSON.stringify({
          error: { code: 'admin_session_expired', message: 'admin authentication required' },
        }),
        { status: 401, headers: { 'content-type': 'application/json' } },
      );
    });
    setAdminAccessToken('expired-token', 600);

    await expect(
      api.chatStream(input, { onTextDelta: () => {}, onDone: () => {}, onError: () => {} }),
    ).rejects.toMatchObject({
      name: 'ApiError',
      status: 401,
      // A catalog key, so the page translates it in the operator's language.
      message: 'Your admin session has expired. Please sign in again.',
    });
    expect(authEvents.length).toBeGreaterThan(0);
    expect(calls).toEqual([
      '/api/v1/admin/workspace/chat/stream',
      '/api/v1/admin/auth/refresh',
    ]);
  });

  test('a provider failure keeps the relay detail instead of a generic message', async () => {
    const detail = "provider 'opencode-go' was not found or is being deleted";
    vi.spyOn(globalThis, 'fetch').mockImplementation(
      async () =>
        new Response(JSON.stringify({ error: { code: '', message: detail } }), {
          status: 404,
          headers: { 'content-type': 'application/json' },
        }),
    );

    const failure = await api
      .chatStream(input, { onTextDelta: () => {}, onDone: () => {}, onError: () => {} })
      .catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).message).toBe(detail);
    expect((failure as ApiError).status).toBe(404);
  });
});
