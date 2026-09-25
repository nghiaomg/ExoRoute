<script lang="ts">
  import { Portal } from '@ark-ui/svelte/portal';
  import { Check, X } from '@lucide/svelte';
  import { type Translate } from '../../lib/format';

  export let toast: { tone: 'success' | 'error'; title: string; message: string } | null;
  export let tr: Translate;
  export let onDismiss: () => void;
</script>

{#if toast}
  <Portal>
    <div class="model-test-toast" class:success={toast.tone === 'success'} class:error={toast.tone === 'error'} role="status">
      <span class="model-test-toast-mark">
        {#if toast.tone === 'success'}
          <Check size={15} />
        {:else}
          <X size={15} />
        {/if}
      </span>
      <div class="model-test-toast-copy">
        <strong>{toast.title}</strong>
        <small>{toast.message}</small>
      </div>
      <button class="model-test-toast-dismiss" aria-label={tr('Dismiss notification')} onclick={onDismiss}>
        <X size={14} />
      </button>
    </div>
  </Portal>
{/if}
