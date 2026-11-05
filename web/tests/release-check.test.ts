/**
 * Unit tests for the release update check (src/lib/release-check.ts).
 *
 * The check is best-effort UI sugar: every failure path must return null and
 * never throw, at most one request per hour may leave the browser, and a
 * dismissed version must stay dismissed until a newer release appears. These
 * tests stub `fetch` and `localStorage` so no network or real storage runs.
 */
import { strict as assert } from 'node:assert/strict';
import test from 'node:test';

interface Storage {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
  removeItem?: (key: string) => void;
}

function installStorage(): Map<string, string> {
  const storage = new Map<string, string>();
  Object.defineProperty(globalThis, 'localStorage', {
    value: {
      getItem: (key: string) => (storage.has(key) ? storage.get(key)! : null),
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
      clear: () => storage.clear(),
    },
    configurable: true,
    writable: true,
  });
  return storage;
}

function releaseResponse(tag: string, url = `https://github.com/nghiaomg/ExoRoute/releases/tag/${tag}`): Response {
  return new Response(JSON.stringify({ tag_name: tag, html_url: url }), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

function jsonResponse(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

test('parseVersionTag accepts the v-prefixed semver tags GitHub publishes and rejects everything else', async () => {
  const { parseVersionTag } = await import('../src/lib/release-check');
  assert.deepEqual(parseVersionTag('v0.1.2'), [0, 1, 2]);
  assert.deepEqual(parseVersionTag('0.2.10'), [0, 2, 10]);
  assert.deepEqual(parseVersionTag('  v1.0.0  '), [1, 0, 0]);
  assert.equal(parseVersionTag('v1.0.0-rc1'), null, 'build suffixes are not comparable releases');
  assert.equal(parseVersionTag('v1.0'), null);
  assert.equal(parseVersionTag('latest'), null);
  assert.equal(parseVersionTag(''), null);
  assert.equal(parseVersionTag('__proto__'), null);
  assert.equal(parseVersionTag(undefined as unknown as string), null);
});

test('isNewerRelease compares numerically and never flags equal or older tags', async () => {
  const { isNewerRelease } = await import('../src/lib/release-check');
  assert.equal(isNewerRelease('0.1.2', 'v0.1.3'), true, 'patch bump');
  assert.equal(isNewerRelease('0.1.2', 'v0.2.0'), true, 'minor bump');
  assert.equal(isNewerRelease('0.1.2', 'v1.0.0'), true, 'major bump');
  assert.equal(isNewerRelease('0.1.2', 'v0.1.2'), false, 'same version');
  assert.equal(isNewerRelease('0.1.2', 'v0.1.1'), false, 'older patch');
  assert.equal(isNewerRelease('0.10.0', 'v0.9.0'), false, 'numeric, not lexicographic');
  assert.equal(isNewerRelease('0.1.2', 'not-a-version'), false, 'unparseable latest');
  assert.equal(isNewerRelease('dev', 'v0.1.3'), false, 'unparseable current');
});

test('fetchLatestRelease maps the GitHub payload and stays silent on every failure path', async () => {
  const { fetchLatestRelease } = await import('../src/lib/release-check');

  const ok = await fetchLatestRelease({
    fetchImpl: (async () => releaseResponse('v0.1.3')) as typeof fetch,
  });
  assert.deepEqual(ok, { latest: '0.1.3', url: 'https://github.com/nghiaomg/ExoRoute/releases/tag/v0.1.3' });

  assert.equal(
    await fetchLatestRelease({ fetchImpl: (async () => jsonResponse({ message: 'Not Found' }, 404)) as typeof fetch }),
    null,
    'HTTP failures (private repo, rate limit) return null',
  );
  assert.equal(
    await fetchLatestRelease({ fetchImpl: (async () => jsonResponse('not an object')) as typeof fetch }),
    null,
    'a JSON scalar is not a release',
  );
  assert.equal(
    await fetchLatestRelease({ fetchImpl: (async () => jsonResponse({ tag_name: 'v1', html_url: 'https://github.com/x' })) as typeof fetch }),
    null,
    'an unparseable tag is not a release',
  );
  assert.equal(
    await fetchLatestRelease({
      fetchImpl: (async () => jsonResponse({ tag_name: 'v0.1.3', html_url: 'http://evil.example.com/release' })) as typeof fetch,
    }),
    null,
    'non-GitHub release URLs are rejected',
  );
  assert.equal(
    await fetchLatestRelease({ fetchImpl: (async () => { throw new Error('offline'); }) as unknown as typeof fetch }),
    null,
    'network failures return null',
  );

  // A stalled response must be cut off by the abort timeout.
  const stalled = await fetchLatestRelease({
    fetchImpl: ((_url: RequestInfo | URL, init?: RequestInit) =>
      new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener('abort', () => reject(new Error('aborted')));
      })) as unknown as typeof fetch,
    timeoutMs: 15,
  });
  assert.equal(stalled, null, 'a timed-out check returns null');
});

function stubCheck(calls: string[], latest: string | null, url: string | null = null) {
  return (async (input: RequestInfo | URL) => {
    calls.push(String(input));
    if (latest === null) return jsonResponse('Not Found', 404);
    return releaseResponse(latest, url ?? `https://github.com/nghiaomg/ExoRoute/releases/tag/v${latest}`);
  }) as unknown as typeof fetch;
}

test('checkForReleaseUpdate notifies only for a newer, not-dismissed release', async () => {
  const { checkForReleaseUpdate, dismissRelease } = await import('../src/lib/release-check');
  installStorage();
  const calls: string[] = [];
  const options = { fetchImpl: stubCheck(calls, '0.1.3'), currentVersion: '0.1.2' };

  const update = await checkForReleaseUpdate(options);
  assert.deepEqual(update, { latest: '0.1.3', url: 'https://github.com/nghiaomg/ExoRoute/releases/tag/v0.1.3' });

  dismissRelease('0.1.3');
  const dismissed = await checkForReleaseUpdate({ ...options, forceRefresh: true });
  assert.equal(dismissed, null, 'a dismissed version stops the notice');

  const newer = await checkForReleaseUpdate({ ...options, fetchImpl: stubCheck(calls, '0.1.4'), forceRefresh: true });
  assert.deepEqual(newer?.latest, '0.1.4', 'a newer release re-notifies after dismissal');
});

test('checkForReleaseUpdate stays quiet when the dashboard is current or the check fails', async () => {
  const { checkForReleaseUpdate } = await import('../src/lib/release-check');
  installStorage();

  const same = await checkForReleaseUpdate({
    fetchImpl: stubCheck([], '0.1.2'),
    currentVersion: '0.1.2',
  });
  assert.equal(same, null, 'equal versions do not notify');

  const older = await checkForReleaseUpdate({
    fetchImpl: stubCheck([], '0.1.1'),
    currentVersion: '0.1.2',
  });
  assert.equal(older, null, 'older releases do not notify');

  const failed = await checkForReleaseUpdate({
    fetchImpl: stubCheck([], null),
    currentVersion: '0.1.2',
  });
  assert.equal(failed, null, 'a failed check does not notify');
});

test('checkForReleaseUpdate reuses the stored result for an hour instead of refetching', async () => {
  const { checkForReleaseUpdate } = await import('../src/lib/release-check');
  installStorage();
  const calls: string[] = [];
  let clock = 1_000_000;
  const now = () => clock;
  const options = { fetchImpl: stubCheck(calls, '0.1.3'), currentVersion: '0.1.2', now };

  const first = await checkForReleaseUpdate(options);
  assert.deepEqual(first?.latest, '0.1.3');
  assert.equal(calls.length, 1);

  const second = await checkForReleaseUpdate(options);
  assert.deepEqual(second?.latest, '0.1.3', 'the stored result is reused within the interval');
  assert.equal(calls.length, 1, 'no second request within the hour');

  clock += 60 * 60 * 1000 + 1;
  const third = await checkForReleaseUpdate(options);
  assert.deepEqual(third?.latest, '0.1.3');
  assert.equal(calls.length, 2, 'the interval expiry allows one fresh request');
});

test('corrupt stored state is ignored instead of throwing', async () => {
  const { checkForReleaseUpdate } = await import('../src/lib/release-check');
  const storage = installStorage();
  storage.set('exoroute.release-check', '{not json');
  storage.set('exoroute.dismissed-release', 'v0.9.9');

  const update = await checkForReleaseUpdate({
    fetchImpl: stubCheck([], '0.1.3'),
    currentVersion: '0.1.2',
  });
  assert.deepEqual(update?.latest, '0.1.3', 'corrupt storage falls back to a fresh check');
});
