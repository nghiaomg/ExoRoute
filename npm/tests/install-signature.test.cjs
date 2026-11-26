'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

// The pre-fix installer auto-runs main() on require; keep it off the network so
// the reproduction fails for the missing verification seam, not for a download.
const originalFetch = globalThis.fetch;
globalThis.fetch = async () => {
  throw new Error('network disabled in unit tests');
};
const installer = require('../scripts/install.cjs');

const ISSUER = 'https://token.actions.githubusercontent.com';
const IDENTITY_PREFIX =
  'https://github.com/nghiaomg/ExoRoute/.github/workflows/release.yml@refs/tags/';
const LATEST_IDENTITY_REGEXP =
  '^https://github[.]com/nghiaomg/ExoRoute/[.]github/workflows/release[.]yml@refs/tags/v[0-9][0-9A-Za-z.+-]*$';
const VALID_BUNDLE = JSON.stringify({ dsseEnvelope: { payload: 'e30=' } });

test.after(() => {
  globalThis.fetch = originalFetch;
});

function temporaryFixture(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'exoroute-npm-test-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  return directory;
}

function writeFixture(directory, name, content) {
  const filePath = path.join(directory, name);
  fs.writeFileSync(filePath, content);
  return filePath;
}

function manifestAndBundle(t) {
  const directory = temporaryFixture(t);
  return {
    directory,
    manifest: writeFixture(directory, 'SHA256SUMS', 'a'.repeat(64) + '  exoroute-linux-x86_64.tar.gz\n'),
    bundle: writeFixture(directory, 'SHA256SUMS.sigstore.json', VALID_BUNDLE)
  };
}

/** Injectable cosign boundary: records every invocation and returns scripted results. */
function fakeCosign(overrides = {}) {
  const calls = [];
  const run = async (file, args, options) => {
    calls.push({ file, args, options });
    assert.equal(file, 'cosign');
    assert.ok(Array.isArray(args), 'cosign arguments must be an array, never a shell string');
    if (args[0] === 'version') {
      return overrides.version
        ? overrides.version()
        : { stdout: 'GitVersion:\tv3.1.3\n', stderr: '' };
    }
    if (args[0] === 'verify-blob') {
      return overrides.verifyBlob ? overrides.verifyBlob(args) : { stdout: '', stderr: '' };
    }
    throw new Error(`unexpected cosign invocation: ${args[0]}`);
  };
  return { run, calls };
}

function verifyCall(calls) {
  const call = calls.find((entry) => entry.args[0] === 'verify-blob');
  assert.ok(call, 'cosign verify-blob was not invoked');
  return call;
}

test('a valid bundle is verified against the exact release-workflow identity for a pinned tag', async (t) => {
  const { manifest, bundle } = manifestAndBundle(t);
  const { run, calls } = fakeCosign();

  await installer.verifyManifestSignature(bundle, manifest, 'v0.1.2', { run });

  assert.deepEqual(verifyCall(calls).args, [
    'verify-blob',
    manifest,
    '--bundle',
    bundle,
    '--certificate-identity',
    IDENTITY_PREFIX + 'v0.1.2',
    '--certificate-oidc-issuer',
    ISSUER
  ]);
});

test('latest uses the tagged-release workflow identity regexp', async (t) => {
  const { manifest, bundle } = manifestAndBundle(t);
  const { run, calls } = fakeCosign();

  await installer.verifyManifestSignature(bundle, manifest, 'latest', { run });

  assert.deepEqual(verifyCall(calls).args, [
    'verify-blob',
    manifest,
    '--bundle',
    bundle,
    '--certificate-identity-regexp',
    LATEST_IDENTITY_REGEXP,
    '--certificate-oidc-issuer',
    ISSUER
  ]);
});

test('a tampered signature (non-zero cosign exit) fails closed with an actionable error', async (t) => {
  const { manifest, bundle } = manifestAndBundle(t);
  const { run } = fakeCosign({
    verifyBlob: () => {
      throw Object.assign(new Error('signature mismatch'), { code: 1 });
    }
  });

  await assert.rejects(
    () => installer.verifyManifestSignature(bundle, manifest, 'v0.1.2', { run }),
    /The release checksum signature is invalid or was issued by an untrusted workflow\./
  );
});

test('a tag that would forge a foreign workflow identity is rejected before cosign runs', async (t) => {
  const { manifest, bundle } = manifestAndBundle(t);
  const { run, calls } = fakeCosign();

  await assert.rejects(
    () => installer.verifyManifestSignature(bundle, manifest, 'v0.1.2@attacker.example', { run }),
    /invalid release tag/
  );
  assert.equal(calls.some((entry) => entry.args[0] === 'verify-blob'), false);
});

test('a missing signature bundle fails closed', async (t) => {
  const { directory, manifest } = manifestAndBundle(t);
  const { run } = fakeCosign();

  await assert.rejects(
    () =>
      installer.verifyManifestSignature(
        path.join(directory, 'MISSING.sigstore.json'),
        manifest,
        'v0.1.2',
        { run }
      ),
    /signature bundle .*missing or malformed/
  );
});

test('empty and malformed bundles fail closed before invoking cosign verify-blob', async (t) => {
  const { directory, manifest } = manifestAndBundle(t);
  const { run, calls } = fakeCosign();
  const empty = writeFixture(directory, 'empty.json', '');
  const malformed = writeFixture(directory, 'bad.json', '{not json');

  await assert.rejects(
    () => installer.verifyManifestSignature(empty, manifest, 'v0.1.2', { run }),
    /signature bundle .*missing or malformed/
  );
  await assert.rejects(
    () => installer.verifyManifestSignature(malformed, manifest, 'v0.1.2', { run }),
    /signature bundle .*missing or malformed/
  );
  assert.equal(calls.some((entry) => entry.args[0] === 'verify-blob'), false);
});

test('a bundle over the 1 MiB limit is rejected (size boundary)', async (t) => {
  const { directory, manifest } = manifestAndBundle(t);
  const { run } = fakeCosign();
  const oversized = writeFixture(directory, 'big.json', 'x'.repeat(1024 * 1024 + 1));

  await assert.rejects(
    () => installer.verifyManifestSignature(oversized, manifest, 'v0.1.2', { run }),
    /signature bundle .*exceeds|missing or malformed/
  );
});

test('cosign missing from the machine fails closed with install instructions', async (t) => {
  const { manifest, bundle } = manifestAndBundle(t);
  const { run } = fakeCosign({
    version: () => {
      throw Object.assign(new Error('spawn cosign ENOENT'), { code: 'ENOENT' });
    }
  });

  await assert.rejects(
    () => installer.verifyManifestSignature(bundle, manifest, 'v0.1.2', { run }),
    /Sigstore Cosign v3\.1\.3 or newer is required.*Install Cosign, then run this installer again\./
  );
});

test('unreadable, unparseable and too-old cosign versions fail closed (3.1.3 boundary)', async (t) => {
  const { manifest, bundle } = manifestAndBundle(t);
  const cases = [
    [{ version: () => { throw new Error('boom'); } }, /Could not read the installed Cosign version\./],
    [{ version: () => ({ stdout: 'unknown\n' }) }, /Could not parse the installed Cosign version\./],
    [{ version: () => ({ stdout: 'GitVersion:\tv3.1.2\n' }) }, /Cosign v3\.1\.3 or newer is required/]
  ];
  for (const [overrides, expected] of cases) {
    const { run } = fakeCosign(overrides);
    await assert.rejects(
      () => installer.verifyManifestSignature(bundle, manifest, 'v0.1.2', { run }),
      expected
    );
  }
  const newer = fakeCosign({ version: () => ({ stdout: 'GitVersion:\tv4.0.0\n' }) });
  await installer.verifyManifestSignature(bundle, manifest, 'v0.1.2', { run: newer.run });
});

test('independent verifications run concurrently without cross-talk', async (t) => {
  const { manifest, bundle } = manifestAndBundle(t);
  const first = fakeCosign();
  const second = fakeCosign();

  await Promise.all([
    installer.verifyManifestSignature(bundle, manifest, 'v0.1.2', { run: first.run }),
    installer.verifyManifestSignature(bundle, manifest, 'v0.1.3', { run: second.run })
  ]);

  assert.equal(first.calls.filter((entry) => entry.args[0] === 'verify-blob').length, 1);
  assert.equal(second.calls.filter((entry) => entry.args[0] === 'verify-blob').length, 1);
  assert.ok(verifyCall(first.calls).args.includes(IDENTITY_PREFIX + 'v0.1.2'));
  assert.ok(verifyCall(second.calls).args.includes(IDENTITY_PREFIX + 'v0.1.3'));
});

function flowFixture() {
  const archiveName = 'exoroute-linux-x86_64.tar.gz';
  const archiveBytes = 'fake-archive-bytes';
  const hash = crypto.createHash('sha256').update(archiveBytes).digest('hex');
  const urls = [];

  async function download(url, destination, maximumBytes) {
    urls.push({ url, destination, maximumBytes });
    switch (path.basename(destination)) {
      case archiveName:
        fs.writeFileSync(destination, archiveBytes);
        break;
      case 'SHA256SUMS':
        fs.writeFileSync(destination, `${hash}  ${archiveName}\n`);
        break;
      case 'SHA256SUMS.sigstore.json':
        fs.writeFileSync(destination, VALID_BUNDLE);
        break;
      default:
        throw new Error(`unexpected download: ${destination}`);
    }
  }

  async function extract(_archive, destination) {
    fs.writeFileSync(path.join(destination, 'exoroute'), 'binary');
    fs.writeFileSync(path.join(destination, 'LICENSE'), 'license');
    fs.writeFileSync(path.join(destination, 'README.md'), 'readme');
  }

  const target = { archive: archiveName, format: 'tar.gz', binary: 'exoroute' };
  return { urls, download, extract, target, archiveName };
}

test('main verifies the signed manifest before extracting or installing', async () => {
  const fixture = flowFixture();
  const events = [];
  const { run } = fakeCosign();
  let installCalls = 0;

  await installer.main({
    download: async (url, destination, maximumBytes) => {
      events.push(`download:${path.basename(destination)}`);
      await fixture.download(url, destination, maximumBytes);
    },
    verify: async (bundle, manifest, tag) => {
      events.push('verify');
      await installer.verifyManifestSignature(bundle, manifest, tag, { run });
    },
    extract: async (...args) => {
      events.push('extract');
      await fixture.extract(...args);
    },
    install: async () => {
      events.push('install');
      installCalls += 1;
    },
    tag: 'v0.1.2',
    target: fixture.target
  });

  assert.equal(installCalls, 1);
  assert.ok(events.indexOf('verify') >= 0, 'signature verification never ran');
  assert.ok(events.indexOf('verify') < events.indexOf('extract'));
  assert.ok(events.indexOf('verify') < events.indexOf('install'));
  assert.ok(
    fixture.urls.some(
      (entry) =>
        entry.url ===
          'https://github.com/nghiaomg/ExoRoute/releases/download/v0.1.2/SHA256SUMS.sigstore.json' &&
        entry.maximumBytes === 1024 * 1024
    ),
    'the Sigstore bundle was not downloaded from the pinned release with a 1 MiB cap'
  );
});

test('main uses real signature verification by default and fails closed on an untrusted signature', async () => {
  const fixture = flowFixture();
  let extracted = false;
  let installed = false;

  await assert.rejects(
    () =>
      installer.main({
        download: fixture.download,
        extract: async () => {
          extracted = true;
        },
        install: async () => {
          installed = true;
        },
        tag: 'v0.1.2',
        target: fixture.target
      }),
    /Cosign|signature is invalid|signature bundle/
  );
  assert.equal(extracted, false, 'the archive was extracted without a verified signature');
  assert.equal(installed, false, 'a binary was installed without a verified signature');
});

test('main aborts before extraction and install when verification fails', async () => {
  const fixture = flowFixture();
  const events = [];

  await assert.rejects(
    () =>
      installer.main({
        download: fixture.download,
        verify: async () => {
          events.push('verify');
          throw new Error('signature verification failed');
        },
        extract: async () => {
          events.push('extract');
        },
        install: async () => {
          events.push('install');
        },
        tag: 'v0.1.2',
        target: fixture.target
      }),
    /signature verification failed/
  );

  assert.deepEqual(events, ['verify']);
});

test('a failed attempt does not suppress verification on a later retry', async () => {
  const fixture = flowFixture();
  let attempts = 0;

  async function run(shouldFail) {
    return installer.main({
      download: fixture.download,
      verify: async (bundle, manifest, tag, options) => {
        attempts += 1;
        const { run: cosign } = fakeCosign(
          shouldFail
            ? { verifyBlob: () => { throw Object.assign(new Error('bad'), { code: 1 }); } }
            : {}
        );
        await installer.verifyManifestSignature(bundle, manifest, tag, { run: cosign });
      },
      extract: fixture.extract,
      install: async () => {},
      tag: 'v0.1.2',
      target: fixture.target
    });
  }

  await assert.rejects(() => run(true), /untrusted workflow/);
  await run(false);
  assert.equal(attempts, 2);
});
