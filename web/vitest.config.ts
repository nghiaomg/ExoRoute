import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';

export default defineConfig({
  plugins: [
    svelte(),
    svelteTesting(),
  ],
  test: {
    environment: 'jsdom',
    include: ['tests/ui/**/*.ui.test.ts'],
    setupFiles: ['./tests/ui/setup.ts'],
    maxWorkers: 1,
    minWorkers: 1,
  },
});
