<script lang="ts">
  import { Portal } from '@ark-ui/svelte/portal';
  import { Check, X } from '@lucide/svelte';
  import { type Translate } from '../lib/format';

  export let toast: { tone: 'success' | 'error'; title: string; message?: string } | null = null;
  export let tr: Translate;
  export let onDismiss: () => void = () => {};
</script>

{#if toast}
  <Portal>
    <div
      class="app-toast"
      class:success={toast.tone === 'success'}
      class:error={toast.tone === 'error'}
      class:compact={!toast.message}
      role="status"
    >
      <span class="app-toast-mark">
        {#if toast.tone === 'success'}
          <Check size={15} />
        {:else}
          <X size={15} />
        {/if}
      </span>
      <div class="app-toast-copy">
        <strong>{toast.title}</strong>
        {#if toast.message}<small>{toast.message}</small>{/if}
      </div>
      <button class="app-toast-dismiss" aria-label={tr('Dismiss notification')} onclick={onDismiss}>
        <X size={14} />
      </button>
    </div>
  </Portal>
{/if}
