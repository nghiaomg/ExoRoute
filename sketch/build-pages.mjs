// Builds the standalone page mockups in sketch/ from shared parts.
// Usage: node build-pages.mjs [pageId ...]   (default: all pages)
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const read = (p) => readFileSync(join(root, p), 'utf8');

const head = read('parts/head.html');
const shellTop = read('parts/shell-top.html');
const shellBottom = read('parts/shell-bottom.html');

const pages = [
  { id: 'overview', title: 'ExoRoute — Overview' },
  { id: 'providers', title: 'ExoRoute — Providers' },
  { id: 'provider', title: 'ExoRoute — Provider Settings' },
  { id: 'combos', title: 'ExoRoute — Combos' },
  { id: 'quota', title: 'ExoRoute — Quota' },
  { id: 'requests', title: 'ExoRoute — Requests' },
  { id: 'chat', title: 'ExoRoute — Chat' },
  { id: 'apikeys', title: 'ExoRoute — API Keys' },
  { id: 'settings', title: 'ExoRoute — Settings' },
  { id: 'docs', title: 'ExoRoute — Docs' },
  { id: 'help', title: 'ExoRoute — Help Center' },
  { id: 'cards', title: 'ExoRoute — Card Variants' },
  { id: 'stats-cards', title: 'ExoRoute — Stat Card Variants' },
];

const only = process.argv.slice(2);
const targets = only.length ? pages.filter((p) => only.includes(p.id)) : pages;

for (const page of targets) {
  const contentPath = `pages/${page.id}.html`;
  if (!existsSync(join(root, contentPath))) {
    console.error(`missing ${contentPath}`);
    continue;
  }
  const cssPath = `pages/${page.id}.css`;
  const css = existsSync(join(root, cssPath))
    ? `    <style>\n${read(cssPath)}\n    </style>`
    : '';
  const html =
    head.replaceAll('{{TITLE}}', page.title).replace('{{PAGE_CSS}}', css) +
    shellTop.replaceAll('{{PAGE_ID}}', page.id) +
    read(contentPath) +
    shellBottom;
  writeFileSync(join(root, `${page.id}.html`), html);
  console.log(`built ${page.id}.html (${html.length} bytes)`);
}
