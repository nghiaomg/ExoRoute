import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const webRoot = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
  plugins: [
    svelte(),
    svelteTesting(),
  ],
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
