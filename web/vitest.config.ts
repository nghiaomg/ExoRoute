import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const webRoot = fileURLToPath(new URL('.', import.meta.url));
// Matches the define in vite.config.ts so components under test see the same
// injected dashboard version the production build uses.
const packageJson = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as {
  version: string;
};

export default defineConfig({
  plugins: [
    svelte(),
    svelteTesting(),
  ],
  define: {
    __APP_VERSION__: JSON.stringify(packageJson.version),
  },
  // The provider and model logos are imported from the repository's `assets`
  // directory, which is outside this project root; without these roots the UI
  // tests fail to transform the imported logo files.
  server: {
    fs: {
      allow: [
        webRoot,
        resolve(webRoot, '../assets/providers'),
        resolve(webRoot, '../assets/models'),
      ],
    },
  },
  test: {
    environment: 'jsdom',
    include: ['tests/ui/**/*.ui.test.ts'],
    setupFiles: ['./tests/ui/setup.ts'],
    maxWorkers: 1,
    minWorkers: 1,
  },
});
