import assert from 'node:assert/strict';
import test from 'node:test';
import { redact, buildDiagnosticReport } from '../src/features/help/diagnostics';
const tr = (k: string) => k;
test('redact strips common credential shapes', () => {
  assert.equal(redact('key=sk-abcdefghijklmnop rest'), 'key=<redacted> rest');
  assert.equal(redact('Authorization: Bearer abcdefghijkl'), 'Authorization: Bearer <redacted>');
  assert.equal(redact('x-api-key=supersecret'), 'x-api-key=<redacted>');
  assert.equal(redact('token=eyJhbGciOi.abc.def'), 'token=<redacted>');
});
test('diagnostic report includes only allow-listed fields', () => {
  const report = buildDiagnosticReport({ overview: { provider_count: 3, combo_count: 2, request_count: 212, dropped_request_logs: 0, uptime_seconds: 660, route_count: 2 }, stats: { average_latency_ms: 16993, success_rate: 96.1 } as never, settings: {}, endpoint: 'http://127.0.0.1:8686/v1?api_key=sk-secret123456', gatewayStatus: 'running', retention: 30, maxRows: 5000 }, tr);
  assert.match(report, /16993 ms/);
  assert.match(report, /212/);
  assert.doesNotMatch(report, /sk-secret123456/);
  assert.match(report, /No secrets are included/);
});
