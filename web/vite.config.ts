import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const webRoot = fileURLToPath(new URL('.', import.meta.url));
const providerAssetsRoot = resolve(webRoot, '../assets/providers');

export default defineConfig({
  plugins: [svelte()],
  build: {
    assetsInlineLimit: 0,
    chunkSizeWarningLimit: 500,
  },
  server: {
    fs: {
      allow: [webRoot, providerAssetsRoot],
    },
    proxy: {
      '/api': {
        target: process.env.EXOROUTE_API_ORIGIN ?? 'http://localhost:8686',
        changeOrigin: true,
      },
    },
  },
});
