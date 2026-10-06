<script lang="ts">
  import { Check, Clipboard, Copy } from '@lucide/svelte';
  import { onDestroy } from 'svelte';
  import type { DocsCodeSample } from './docsCopy';

  export let sample: DocsCodeSample;
  export let copyLabel = 'Copy';
  export let copiedLabel = 'Copied';
  export let copyFailedLabel = 'Could not copy';

  let copied = false;
  let copyFailed = false;
  let resetTimer: number | null = null;

  function clearResetTimer(): void {
    if (resetTimer !== null) window.clearTimeout(resetTimer);
    resetTimer = null;
  }

  async function copyCode(): Promise<void> {
    if (typeof navigator === 'undefined' || !navigator.clipboard) {
      copyFailed = true;
      copied = false;
      return;
    }
    try {
      await navigator.clipboard.writeText(sample.code);
      copied = true;
      copyFailed = false;
    } catch {
      copied = false;
      copyFailed = true;
    }
    clearResetTimer();
    resetTimer = window.setTimeout(() => {
      copied = false;
      copyFailed = false;
      resetTimer = null;
    }, 2200);
  }

  onDestroy(clearResetTimer);
</script>

<div class="docs-code-block">
  <div class="docs-code-toolbar">
    <div class="docs-code-meta">
      <span class="docs-code-dot"></span>
      <span>{sample.label}</span>
      <code>{sample.language}</code>
    </div>
    <button class="docs-copy-button" type="button" onclick={copyCode} aria-label={copied ? copiedLabel : copyLabel}>
      {#if copied}
        <Check size={14} />{copiedLabel}
      {:else if copyFailed}
        <Clipboard size={14} />{copyFailedLabel}
      {:else}
        <Copy size={14} />{copyLabel}
      {/if}
    </button>
  </div>
  <pre><code>{sample.code}</code></pre>
</div>
