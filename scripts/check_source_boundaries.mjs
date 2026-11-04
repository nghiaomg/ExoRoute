import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';

const root = process.cwd();

// The budget is a review trigger, not a formatting target. A file that genuinely
// owns one large responsibility is declared in `oversizedAllowlist` with its
// reason rather than split into pass-through modules to fit the number: AGENTS.md
// settles which split is right, and it is not a line count.
const budget = 800;

// Production files that exceed the budget on purpose. Every entry carries a
// reason and is printed, so keeping a large file is a recorded decision rather
// than an accident. Prefer restructuring; use this only when the file really does
// own the responsibility.
const oversizedAllowlist = new Map([
  // ['src/feature/example.rs', 'Owns the whole example lifecycle because …'],
]);

const sourceRoots = [path.join(root, 'src'), path.join(root, 'web', 'src')];
const extensions = new Set(['.rs', '.ts', '.svelte', '.css']);

async function collect(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const filePath = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await collect(filePath));
    else if (extensions.has(path.extname(entry.name))) files.push(filePath);
  }
  return files;
}

function productionLineCount(text, extension) {
  if (extension !== '.rs') return text.split(/\r?\n/).length;
  const testMarker = text.search(/\n\s*#\[cfg\(test\)\]\s*\n\s*mod tests\s*\{|\n\s*mod tests\s*\{/);
  return (testMarker >= 0 ? text.slice(0, testMarker) : text).split(/\r?\n/).length;
}

function isIntentionalNonProductionFile(filePath, extension) {
  const relative = path.relative(root, filePath).replaceAll(path.sep, '/');
  // A split test module is either a single `tests.rs` or a `tests/` directory
  // holding one file per concern; neither is production code.
  if (extension === '.rs' && (relative.endsWith('/tests.rs') || relative.includes('/tests/'))) return true;
  return extension === '.ts' && relative.startsWith('web/src/lib/locales/');
}

const files = (await Promise.all(sourceRoots.map(collect))).flat();
const oversized = [];
const allowed = [];
const excluded = [];
const relativePath = (filePath) => path.relative(root, filePath).replaceAll(path.sep, '/');
const relativeFiles = new Set(files.map(relativePath));
const architectureIssues = [];

// The preset catalog's `adapter_id` is not the directory name — several presets
// share one adapter, and a few adapters expose no preset — so this list cannot be
// derived from it. It is instead closed against the filesystem below, which is
// what makes a new provider unable to be added with its module rule unchecked.
const providerModules = [
  'antigravity',
  'cline',
  'cloud_api',
  'codex',
  'command_code',
  'freebuff',
  'generic',
  'kilo',
  'kilocode',
  'muse',
  'nvidia_nim',
  'opencode',
  'openrouter',
  'qoder',
];

for (const provider of providerModules) {
  const modulePath = `src/provider_adapters/${provider}/mod.rs`;
  if (!relativeFiles.has(modulePath)) {
    architectureIssues.push(`provider adapter is missing its directory module: ${modulePath}`);
  }
}

// The reverse check: every adapter directory must be listed above, otherwise a
// new provider silently escapes the one-directory-module rule. A hand-written
// list had already drifted here once.
const adapterRoot = path.join(root, 'src', 'provider_adapters');
const listedProviders = new Set(providerModules);
for (const entry of await readdir(adapterRoot, { withFileTypes: true })) {
  if (!entry.isDirectory() || entry.name === 'auth' || entry.name === 'tests') continue;
  if (!listedProviders.has(entry.name)) {
    architectureIssues.push(`provider adapter directory is not listed in providerModules, so its module rule is unchecked: src/provider_adapters/${entry.name}`);
  }
}

for (const filePath of files) {
  const relative = relativePath(filePath);
  if (relative.endsWith('_tests.rs')) {
    architectureIssues.push(`extracted test modules must be named tests.rs inside a feature directory: ${relative}`);
  }
  if (relative.startsWith('src/') && relative.endsWith('.rs') && !relative.endsWith('/mod.rs')) {
    const moduleDirectory = relative.slice(0, -'.rs'.length);
    if (relativeFiles.has(`${moduleDirectory}/mod.rs`)) {
      architectureIssues.push(`Rust module has both a legacy root file and directory module: ${relative}`);
    }
  }
}

for (const filePath of files) {
  const extension = path.extname(filePath);
  if (isIntentionalNonProductionFile(filePath, extension)) {
    excluded.push(filePath);
    continue;
  }
  const relative = relativePath(filePath);
  const text = await readFile(filePath, 'utf8');
  const lines = productionLineCount(text, extension);
  if (lines <= budget) continue;
  const reason = oversizedAllowlist.get(relative);
  if (reason) allowed.push(`${relative}: ${lines} lines — ${reason}`);
  else oversized.push({ filePath, lines });
}

oversized.sort((left, right) => right.lines - left.lines);
if (oversized.length > 0) {
  console.warn(`Source boundary warnings: ${oversized.length} file(s) exceed the ${budget}-line production budget.`);
  for (const { filePath, lines } of oversized) {
    console.warn(` - ${path.relative(root, filePath)}: ${lines} lines`);
  }
  console.warn('Restructure the file, or record why it owns the responsibility in oversizedAllowlist.');
}
if (allowed.length > 0) {
  console.info(`Acknowledged oversized files: ${allowed.length} declared in oversizedAllowlist.`);
  for (const entry of allowed) console.info(` - ${entry}`);
}
if (excluded.length > 0) {
  console.info(`Boundary exemptions: ${excluded.length} test-only or locale catalog files are outside the production budget.`);
}
if (architectureIssues.length > 0) {
  console.warn(`Architecture boundary warnings: ${architectureIssues.length} issue(s).`);
  for (const issue of architectureIssues) console.warn(` - ${issue}`);
}
if (oversized.length > 0 || architectureIssues.length > 0) process.exitCode = 1;
