/// <reference types="vite/client" />

/** Injected by vite.config.ts from package.json; see src/lib/version.ts. */
declare const __APP_VERSION__: string;

declare module '*.svelte' {
  import type { Component } from 'svelte';
  const component: Component;
  export default component;
}
