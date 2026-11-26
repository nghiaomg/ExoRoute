<script lang="ts">
  import { Check, Copy } from '@lucide/svelte';

  interface Props {
    value: string;
    copyLabel?: string;
    copiedLabel?: string;
    class?: string;
  }

  let {
    value,
    copyLabel = 'Copy key',
    copiedLabel = 'Copied!',
    class: className = '',
  }: Props = $props();

  let copied = $state(false);
  let resetTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    return () => {
      if (resetTimer) clearTimeout(resetTimer);
    };
  });

  function copy(): void {
    void navigator.clipboard.writeText(value).then(() => {
      copied = true;
      if (resetTimer) clearTimeout(resetTimer);
      resetTimer = setTimeout(() => {
        copied = false;
        resetTimer = undefined;
      }, 1500);
    }).catch(() => {
      // Clipboard permission denied or unsupported: keep the idle state, as
      // the previous Ark clipboard did (no error surface existed).
    });
  }
</script>

<button
  type="button"
  class="secondary-button clipboard-trigger {className}"
  onclick={copy}
>
  {#if copied}
    <Check size={14} class="dark-clipboard-check" />
    <span>{copiedLabel}</span>
  {:else}
    <Copy size={14} />
    <span>{copyLabel}</span>
  {/if}
</button>
