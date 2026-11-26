/**
 * CSV serialization for the Requests page export (request.csv.ts).
 *
 * The verifier-facing guarantees: RFC 4180 quoting, masked columns stay
 * masked in the file, provider credentials never leave the backend
 * boundary, and the header stays stable so Excel/Sheets parse it first try.
 */
import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import {
  escapeCsvField,
  requestsCsvFilename,
  requestsToCsv,
} from '../src/features/requests/request.csv';
import type { RequestLog } from '../src/lib/types';
import { DEFAULT_MASKED_COLUMNS } from '../src/features/requests/request.mask';

function row(overrides: Partial<RequestLog> = {}): RequestLog {
  return {
    request_id: 'req-1',
    api_key_id: 'key-1',
    api_key_name: 'My Key',
    model: 'gpt-4o-mini',
    route_alias: 'fast',
    provider_id: 'openai',
    client_protocol: 'responses',
    upstream_protocol: 'chat',
    status: 200,
    duration_ms: 431,
    input_tokens: 1200,
    output_tokens: 340,
    created_at: '2026-11-06 12:00:00',
    ...overrides,
  };
}

test('requestsToCsv emits a stable header and one line per row', () => {
  const csv = requestsToCsv([row(), row({ request_id: 'req-2' })]);
  const lines = csv.split('\n');
  assert.equal(
    lines[0],
    'created_at,request_id,api_key_id,api_key_name,model,route_alias,provider_id,client_protocol,upstream_protocol,status,duration_ms,input_tokens,output_tokens,cached_tokens,cache_input_tokens,cost_micro_usd,error',
  );
  assert.equal(lines.length, 3);
  assert.match(lines[1], /req-1/);
  assert.match(lines[2], /req-2/);
});

test('escapeCsvField quotes per RFC 4180', () => {
  assert.equal(escapeCsvField('plain'), 'plain');
  assert.equal(escapeCsvField('with,comma'), '"with,comma"');
  assert.equal(escapeCsvField('with"quote'), '"with""quote"');
  assert.equal(escapeCsvField('line\nbreak'), '"line\nbreak"');
  assert.equal(escapeCsvField('semi;colon'), '"semi;colon"');
});

test('masked columns stay masked in the exported file', () => {
  const csv = requestsToCsv([row()], {
    maskedColumns: { ...DEFAULT_MASKED_COLUMNS, key: true, model: true, alias: true, provider: true },
  });
  assert.doesNotMatch(csv, /My Key|gpt-4o-mini|openai/, 'sensitive identifiers must not export');
  assert.match(csv, /\*\*\*/, 'masked values export as asterisks');
});

test('provider credentials are never exported', () => {
  const secret = row({ provider_credential_id: 'cred-secret-123' });
  const csv = requestsToCsv([secret]);
  assert.doesNotMatch(csv, /cred-secret-123/);
});

test('missing values export as empty fields, not "undefined"', () => {
  const minimal: RequestLog = { model: 'm', created_at: '2026-11-06 12:00:00' };
  const csv = requestsToCsv([minimal]);
  const cells = csv.split('\n')[1].split(',');
  assert.equal(cells.length, 17, 'every header column still gets a field');
  assert.equal(cells[3], '', 'api_key_name');
  assert.doesNotMatch(csv, /undefined|null/);
});

test('error text with commas and newlines is quoted as one field', () => {
  const csv = requestsToCsv([row({ error: 'upstream said "no, not yet"\nretry later', status: 500 })]);
  assert.match(csv, /"upstream said ""no, not yet""\nretry later"/);
});

test('requestsCsvFilename stamps sortably without colons or dots', () => {
  const name = requestsCsvFilename(new Date('2026-11-06T12:00:00.000Z'));
  assert.equal(name, 'exoroute-requests-2026-11-06T12-00-00.csv');
});
