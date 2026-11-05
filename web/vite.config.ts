import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const webRoot = fileURLToPath(new URL('.', import.meta.url));
const providerAssetsRoot = resolve(webRoot, '../assets/providers');
const modelAssetsRoot = resolve(webRoot, '../assets/models');
// The dashboard version is injected from package.json so the UI, the release
// check and the published package always report the same version.
const packageJson = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as {
  version: string;
};

export default defineConfig({
  plugins: [svelte()],
  define: {
    __APP_VERSION__: JSON.stringify(packageJson.version),
  },
  build: {
    assetsInlineLimit: 0,
    chunkSizeWarningLimit: 500,
  },
  server: {
    fs: {
      allow: [webRoot, providerAssetsRoot, modelAssetsRoot],
    },
    proxy: {
      '/api': {
        target: process.env.EXOROUTE_API_ORIGIN ?? 'http://localhost:8686',
        // Keep the browser's Host header. The admin auth endpoints reject any
        // POST whose Origin does not match the request Host, so rewriting Host
        // to the API authority made every session refresh and logout from the
        // dev server fail with 403.
        changeOrigin: false,
      },
    },
  },
});
