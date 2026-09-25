import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';

const root = process.cwd();
const budget = 800;
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
const excluded = [];
const relativePath = (filePath) => path.relative(root, filePath).replaceAll(path.sep, '/');
const relativeFiles = new Set(files.map(relativePath));
const architectureIssues = [];
const providerModules = [
  'antigravity',
  'cline',
  'codex',
  'command_code',
  'freebuff',
  'generic',
  'kilo',
  'nvidia_nim',
  'opencode',
  'openrouter',
];

for (const provider of providerModules) {
  const modulePath = `src/provider_adapters/${provider}/mod.rs`;
  if (!relativeFiles.has(modulePath)) {
    architectureIssues.push(`provider adapter is missing its directory module: ${modulePath}`);
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
  const text = await readFile(filePath, 'utf8');
  const lines = productionLineCount(text, extension);
  if (lines > budget) oversized.push({ filePath, lines });
}

oversized.sort((left, right) => right.lines - left.lines);
if (oversized.length > 0) {
  console.warn(`Source boundary warnings: ${oversized.length} file(s) exceed the ${budget}-line production budget.`);
  for (const { filePath, lines } of oversized) {
    console.warn(` - ${path.relative(root, filePath)}: ${lines} lines`);
  }
}
if (excluded.length > 0) {
  console.info(`Boundary exemptions: ${excluded.length} test-only or locale catalog files are outside the production budget.`);
}
if (architectureIssues.length > 0) {
  console.warn(`Architecture boundary warnings: ${architectureIssues.length} issue(s).`);
  for (const issue of architectureIssues) console.warn(` - ${issue}`);
}
if (oversized.length > 0 || architectureIssues.length > 0) process.exitCode = 1;
