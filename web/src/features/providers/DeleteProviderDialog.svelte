<script lang="ts">
  import { LoaderCircle } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import { type Translate } from '../../lib/format';
  import type { Provider } from '../../lib/types';

  export let provider: Provider | null;
  export let deletingId = '';
  export let tr: Translate;
  export let onConfirm: (provider: Provider) => Promise<void>;
  export let onCancel: () => void;
</script>

<ArkDialog
  open={provider !== null}
  role="alertdialog"
  closeLabel={tr('Close dialog')}
  title={tr('Are you absolutely sure?')}
  kicker={tr('EXOROUTE CONTROL PLANE')}
  onClose={onCancel}
>
  <div class="modal-form">
    <p class="modal-description">
      {tr('Delete {name}? This cannot be undone.', { name: provider?.name ?? '' })}
    </p>
    <div class="modal-actions" style="margin-top: 16px;">
      <button type="button" class="secondary-button" disabled={Boolean(deletingId)} onclick={onCancel}>
        {tr('Cancel')}
      </button>
      <button
        type="button"
        class="primary-button danger"
        disabled={Boolean(deletingId)}
        onclick={() => { if (provider) void onConfirm(provider); }}
      >
        {#if deletingId}<LoaderCircle size={14} class="spin" />{tr('Deleting…')}{:else}{tr('Delete provider')}{/if}
      </button>
    </div>
  </div>
</ArkDialog>
