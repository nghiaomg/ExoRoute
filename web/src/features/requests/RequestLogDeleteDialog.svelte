<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { LoaderCircle, Trash2 } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';

  export let tr: Translate;
  export let busy = false;
  export let disabled = false;
  export let onConfirm: () => Promise<boolean>;

  /* Native dialog in the shared .modal shell markup: the trigger opens it,
     [data-close-modal] buttons and backdrop clicks close it through the
     shell's delegated handlers, and Escape closes it natively. */
  let dialogEl: HTMLDialogElement | null = null;

  function openDialog(): void {
    dialogEl?.showModal();
  }

  async function confirm(): Promise<void> {
    if (busy) return;
    if (await onConfirm()) dialogEl?.close();
  }
</script>

<Button type="button" class="secondary-button compact request-log-delete" {disabled} onclick={openDialog}>
  {#if busy}<LoaderCircle size={13} class="spin" />{tr('Deleting request logs…')}{:else}<Trash2 size={13} />{tr('Delete all logs')}{/if}
</Button>

<dialog
  class="modal"
  bind:this={dialogEl}
  role="alertdialog"
  aria-labelledby="request-log-delete-title"
  aria-describedby="request-log-delete-description"
>
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <span class="modal-kicker">{tr('EXOROUTE CONTROL PLANE')}</span>
        <h2 class="modal-title" id="request-log-delete-title">{tr('Are you absolutely sure?')}</h2>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <p class="modal-description" id="request-log-delete-description">
      {tr('Delete all request logs, statistics, and lifetime API key counts? This permanently removes the stored history. Continue?')}
    </p>
    <div class="modal-actions">
      <Button class="secondary-button" type="button" data-close-modal disabled={busy}>
        {tr('Cancel')}
      </Button>
      <Button type="button" class="primary-button danger" disabled={busy} onclick={confirm}>
        {#if busy}<LoaderCircle size={13} class="spin" />{tr('Deleting…')}{:else}<Trash2 size={13} />{tr('Delete all logs')}{/if}
      </Button>
    </div>
  </div>
</dialog>
