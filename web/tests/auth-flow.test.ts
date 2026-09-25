import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import { ApiError } from '../src/lib/api';
import { classifySessionRestore } from '../src/lib/auth-flow';

test('a successful restore reports the password-change requirement only when authenticated', () => {
  assert.deepEqual(
    classifySessionRestore({ authenticated: true, unavailable: false, mustChangePassword: true }),
    { kind: 'ready', authenticated: true, mustChangePassword: true },
  );
  assert.deepEqual(
    classifySessionRestore({ authenticated: true, unavailable: false, mustChangePassword: false }),
    { kind: 'ready', authenticated: true, mustChangePassword: false },
  );
});

test('an anonymous visitor is ready without a forced password change', () => {
  // `mustChangePassword` must never leak through when the refresh was rejected,
  // otherwise the login screen would show the forced-change flow to a stranger.
  assert.deepEqual(
    classifySessionRestore({ authenticated: false, unavailable: false, mustChangePassword: true }),
    { kind: 'ready', authenticated: false, mustChangePassword: false },
  );
});

test('a rate-limited restore keeps the retry delay bounded and classified', () => {
  const outcome = classifySessionRestore({
    authenticated: false,
    unavailable: true,
    mustChangePassword: false,
    error: new ApiError('too many attempts', 429, false, false, '', 12),
  });
  assert.deepEqual(outcome, { kind: 'unavailable', checkStatus: 'rate_limited', retryAfterSeconds: 12 });
});

test('a rate-limited restore without a Retry-After falls back to a short default', () => {
  const outcome = classifySessionRestore({
    authenticated: false,
    unavailable: true,
    mustChangePassword: false,
    error: new ApiError('too many attempts', 429),
  });
  assert.deepEqual(outcome, { kind: 'unavailable', checkStatus: 'rate_limited', retryAfterSeconds: 5 });
});

test('an unreachable API is unavailable but never rate limited', () => {
  const outcome = classifySessionRestore({
    authenticated: false,
    unavailable: true,
    mustChangePassword: false,
    error: new ApiError('could not reach the API', 0),
  });
  assert.deepEqual(outcome, { kind: 'unavailable', checkStatus: 'unavailable', retryAfterSeconds: 0 });
});
