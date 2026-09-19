<script lang="ts">
  import { Dialog } from '@ark-ui/svelte/dialog';
  import { Portal } from '@ark-ui/svelte/portal';
  import { LoaderCircle, Trash2, X } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';

  export let tr: Translate;
  export let busy = false;
  export let disabled = false;
  export let onConfirm: () => Promise<boolean>;

  let open = false;

  async function confirm(): Promise<void> {
    if (busy) return;
    if (await onConfirm()) open = false;
  }
</script>

<Dialog.Root role="alertdialog" bind:open>
  <Dialog.Trigger class="secondary-button compact request-log-delete" disabled={disabled || busy}>
    {#if busy}<LoaderCircle size={13} class="spin" />{tr('Deleting request logs…')}{:else}<Trash2 size={13} />{tr('Delete all logs')}{/if}
  </Dialog.Trigger>
  <Portal>
    <Dialog.Backdrop class="modal-backdrop" />
    <Dialog.Positioner class="modal-positioner">
      <Dialog.Content class="modal-card">
        <div class="modal-header">
          <div>
            <span class="modal-kicker">{tr('EXOROUTE CONTROL PLANE')}</span>
            <Dialog.Title class="modal-title">{tr('Are you absolutely sure?')}</Dialog.Title>
          </div>
          <Dialog.CloseTrigger type="button" class="icon-button" aria-label={tr('Close dialog')}>
            <X size={18} />
          </Dialog.CloseTrigger>
        </div>
        <Dialog.Description class="modal-description">
          {tr('Delete all request logs, statistics, and lifetime API key counts? This permanently removes the stored history. Continue?')}
        </Dialog.Description>
        <div class="modal-actions" style="margin-top: 16px;">
          <Dialog.CloseTrigger class="secondary-button" disabled={busy}>
            {tr('Cancel')}
          </Dialog.CloseTrigger>
          <button type="button" class="primary-button danger" disabled={busy} onclick={confirm}>
            {#if busy}<LoaderCircle size={13} class="spin" />{tr('Deleting…')}{:else}<Trash2 size={13} />{tr('Delete all logs')}{/if}
          </button>
        </div>
      </Dialog.Content>
    </Dialog.Positioner>
  </Portal>
</Dialog.Root>
