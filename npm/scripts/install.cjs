'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const fsp = fs.promises;
const os = require('node:os');
const path = require('node:path');
const { execFile } = require('node:child_process');
const { promisify } = require('node:util');

const execFileAsync = promisify(execFile);
const REPOSITORY = 'nghiaomg/ExoRoute';
const MAX_DOWNLOAD_BYTES = 100 * 1024 * 1024;
const ALLOWED_DOWNLOAD_HOSTS = new Set([
  'github.com',
  'objects.githubusercontent.com',
  'release-assets.githubusercontent.com',
  'github-releases.githubusercontent.com'
]);

const TARGETS = new Map([
  [
    'linux-x64',
    {
      archive: 'exoroute-linux-x86_64.tar.gz',
      format: 'tar.gz',
      binary: 'exoroute'
    }
  ],
  [
    'linux-arm64',
    {
      archive: 'exoroute-linux-aarch64.tar.gz',
      format: 'tar.gz',
      binary: 'exoroute'
    }
  ],
  [
    'darwin-x64',
    {
      archive: 'exoroute-darwin-x86_64.tar.gz',
      format: 'tar.gz',
      binary: 'exoroute'
    }
  ],
  [
    'darwin-arm64',
    {
      archive: 'exoroute-darwin-aarch64.tar.gz',
      format: 'tar.gz',
      binary: 'exoroute'
    }
  ],
  [
    'win32-x64',
    {
      archive: 'exoroute-windows-x86_64.zip',
      format: 'zip',
      binary: 'exoroute.exe'
    }
  ]
]);

const PACKAGE_ROOT = path.resolve(__dirname, '..');
const PACKAGE_METADATA = JSON.parse(
  fs.readFileSync(path.join(PACKAGE_ROOT, 'package.json'), 'utf8')
);
const EXPECTED_FILES = (binary) => [binary, 'LICENSE', 'README.md'];

function targetForCurrentRuntime() {
  const key = `${process.platform}-${process.arch}`;
  const target = TARGETS.get(key);
  if (!target) {
    throw new Error(
      `ExoRoute does not publish a binary for ${process.platform}/${process.arch}.`
    );
  }
  return target;
}

function releaseTag() {
  const requested = (process.env.EXOROUTE_VERSION || `v${PACKAGE_METADATA.version}`).trim();
  if (requested === 'latest') {
    return requested;
  }
  const tag = requested.startsWith('v') ? requested : `v${requested}`;
  if (!/^v[0-9][0-9A-Za-z.+-]*$/.test(tag)) {
    throw new Error('EXOROUTE_VERSION must be latest or a valid release tag.');
  }
  return tag;
}

function releaseUrl(tag, asset) {
  const base = tag === 'latest'
    ? `https://github.com/${REPOSITORY}/releases/latest/download`
    : `https://github.com/${REPOSITORY}/releases/download/${encodeURIComponent(tag)}`;
  return `${base}/${asset}`;
}

async function downloadFile(url, destination, maximumBytes) {
  if (typeof fetch !== 'function') {
    throw new Error('Node.js 18 or newer is required because the installer uses fetch.');
  }
  const response = await fetch(url, { redirect: 'follow' });
  if (!response.ok) {
    throw new Error(`download failed with HTTP ${response.status}: ${url}`);
  }
  const finalUrl = new URL(response.url || url);
  if (finalUrl.protocol !== 'https:' || !ALLOWED_DOWNLOAD_HOSTS.has(finalUrl.hostname)) {
    throw new Error(`release download redirected to an untrusted URL: ${finalUrl}`);
  }
  const contentLength = Number(response.headers.get('content-length'));
  if (Number.isFinite(contentLength) && contentLength > maximumBytes) {
    throw new Error('release download exceeds the configured size limit.');
  }
  if (!response.body) {
    throw new Error('release server returned an empty response stream.');
  }

  const output = await fsp.open(destination, 'wx');
  let total = 0;
  try {
    for await (const chunk of response.body) {
      const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      total += buffer.length;
      if (total > maximumBytes) {
        throw new Error('release download exceeds the configured size limit.');
      }
      await output.write(buffer);
    }
    await output.sync();
  } finally {
    await output.close();
  }
}

async function sha256File(filePath) {
  const hash = crypto.createHash('sha256');
  for await (const chunk of fs.createReadStream(filePath)) {
    hash.update(chunk);
  }
  return hash.digest('hex');
}

async function expectedHash(manifestPath, asset) {
  const lines = (await fsp.readFile(manifestPath, 'utf8')).split(/\r?\n/);
  const matches = lines
    .map((line) => line.trim().split(/\s+/))
    .filter(
      (parts) =>
        parts.length === 2 &&
        /^[a-f0-9]{64}$/i.test(parts[0]) &&
        parts[1] === asset
    );
  if (matches.length !== 1) {
    throw new Error(`checksum manifest does not contain exactly one entry for ${asset}.`);
  }
  return matches[0][0].toLowerCase();
}

function assertExactEntries(entries, expected, archiveType) {
  const actual = entries.filter(Boolean).map((entry) => entry.trim()).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((entry, index) => entry !== wanted[index])) {
    throw new Error(`${archiveType} archive contains unexpected or duplicate paths.`);
  }
}

async function extractTar(archivePath, destination, binary) {
  const { stdout } = await execFileAsync('tar', ['-tzf', archivePath], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024,
    windowsHide: true
  });
  assertExactEntries(stdout.split(/\r?\n/), EXPECTED_FILES(binary), 'tar');
  await execFileAsync('tar', ['-xzf', archivePath, '-C', destination], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024,
    windowsHide: true
  });
}

async function extractZip(archivePath, destination) {
  const scriptPath = path.join(destination, '.extract-release.ps1');
  const script = `
param(
  [Parameter(Mandatory = $true)][string] $Archive,
  [Parameter(Mandatory = $true)][string] $Destination
)
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$expected = @('exoroute.exe', 'LICENSE', 'README.md')
$maximumBytes = [int64]104857600
$zip = [System.IO.Compression.ZipFile]::OpenRead($Archive)
try {
  $entries = @($zip.Entries)
  if ($entries.Count -ne $expected.Count) {
    throw 'zip archive contains an unexpected number of entries'
  }
  [int64]$total = 0
  foreach ($entry in $entries) {
    if ($entry.FullName -notin $expected -or $entry.Length -gt $maximumBytes) {
      throw 'zip archive contains an unexpected or unsafe entry'
    }
    $total += $entry.Length
    if ($total -gt $maximumBytes) {
      throw 'zip archive expands beyond the configured size limit'
    }
    $destinationPath = Join-Path -Path $Destination -ChildPath $entry.FullName
    [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $destinationPath, $false)
  }
}
finally {
  $zip.Dispose()
}
`;
  await fsp.writeFile(scriptPath, script, { encoding: 'utf8', flag: 'wx' });
  const powershell = process.env.SystemRoot
    ? path.join(process.env.SystemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe')
    : 'powershell.exe';
  try {
    await execFileAsync(
      powershell,
      [
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        scriptPath,
        '-Archive',
        archivePath,
        '-Destination',
        destination
      ],
      { encoding: 'utf8', maxBuffer: 64 * 1024, windowsHide: true }
    );
  } finally {
    await fsp.rm(scriptPath, { force: true });
  }
}

async function verifyExtractedFiles(destination, binary) {
  const expected = EXPECTED_FILES(binary);
  const entries = await fsp.readdir(destination);
  assertExactEntries(entries, expected, 'extracted');
  for (const name of expected) {
    const filePath = path.join(destination, name);
    const metadata = await fsp.lstat(filePath);
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > MAX_DOWNLOAD_BYTES) {
      throw new Error(`extracted entry '${name}' is not a safe regular file.`);
    }
  }
}

async function ensureRealDirectory(directory) {
  try {
    const metadata = await fsp.lstat(directory);
    if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
      throw new Error(`npm package directory is not a real directory: ${directory}`);
    }
  } catch (error) {
    if (error.code !== 'ENOENT') {
      throw error;
    }
    await fsp.mkdir(directory, { recursive: true });
  }
}

async function installBinary(binary, sourcePath) {
  const vendorDirectory = path.join(PACKAGE_ROOT, 'vendor');
  await ensureRealDirectory(vendorDirectory);
  const destination = path.join(vendorDirectory, binary);
  const temporary = path.join(
    vendorDirectory,
    `.exoroute-${process.pid}-${Date.now().toString(36)}.tmp`
  );
  await fsp.copyFile(sourcePath, temporary);
  try {
    if (process.platform !== 'win32') {
      await fsp.chmod(temporary, 0o755);
    }
    try {
      const existing = await fsp.lstat(destination);
      if (!existing.isFile() || existing.isSymbolicLink()) {
        throw new Error(`existing ExoRoute install is not a regular file: ${destination}`);
      }
      await fsp.rm(destination, { force: true });
    } catch (error) {
      if (error.code !== 'ENOENT') {
        throw error;
      }
    }
    await fsp.rename(temporary, destination);
  } finally {
    await fsp.rm(temporary, { force: true });
  }
}

async function main() {
  const target = targetForCurrentRuntime();
  const tag = releaseTag();
  const temporaryDirectory = await fsp.mkdtemp(path.join(os.tmpdir(), 'exoroute-npm-'));
  const archivePath = path.join(temporaryDirectory, target.archive);
  const manifestPath = path.join(temporaryDirectory, 'SHA256SUMS');
  const extractDirectory = path.join(temporaryDirectory, 'extract');
  await fsp.mkdir(extractDirectory);

  try {
    await downloadFile(
      releaseUrl(tag, target.archive),
      archivePath,
      MAX_DOWNLOAD_BYTES
    );
    await downloadFile(
      releaseUrl(tag, 'SHA256SUMS'),
      manifestPath,
      64 * 1024
    );
    const expected = await expectedHash(manifestPath, target.archive);
    const actual = await sha256File(archivePath);
    if (actual !== expected) {
      throw new Error(`SHA-256 mismatch for ${target.archive}; the archive was not installed.`);
    }

    if (target.format === 'zip') {
      await extractZip(archivePath, extractDirectory);
    } else {
      await extractTar(archivePath, extractDirectory, target.binary);
    }
    await verifyExtractedFiles(extractDirectory, target.binary);
    await installBinary(target.binary, path.join(extractDirectory, target.binary));
    console.log(`Installed ExoRoute ${tag} for ${process.platform}/${process.arch}.`);
  } finally {
    await fsp.rm(temporaryDirectory, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(`ExoRoute npm installation failed: ${error.message}`);
  process.exitCode = 1;
});
