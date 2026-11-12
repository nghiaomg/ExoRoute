<script lang="ts">
  import type { Snippet } from 'svelte';
  import Button from './Button.svelte';

  interface Props {
    open: boolean;
    title: string;
    kicker?: string;
    wide?: boolean;
    preventClose?: boolean;
    closeLabel?: string;
    class?: string;
    role?: 'dialog' | 'alertdialog';
    onClose?: () => void;
    children?: Snippet;
  }

  let {
    open = $bindable(false),
    title,
    kicker = 'EXOROUTE CONTROL PLANE',
    wide = false,
    preventClose = false,
    closeLabel = 'Close dialog',
    class: className = '',
    role = 'dialog',
    onClose = () => {},
    children,
  }: Props = $props();

  let dialogEl = $state<HTMLDialogElement | null>(null);

  /* The dialog is mounted only while `open` (lazyMount/unmountOnExit parity),
     so the effect only has to open it after the element exists. Closing in
     the other direction happens through the native `close` event below, which
     keeps `open` two-way synced; unmounting covers the parent-driven path. */
  $effect(() => {
    if (open && dialogEl && !dialogEl.open) {
      dialogEl.showModal();
    }
  });

  function handleCancel(event: Event): void {
    // Escape key: the native path is blocked for preventClose dialogs; the
    // shell's window-level Escape handler skips [data-prevent-close] dialogs.
    if (preventClose) {
      event.preventDefault();
    }
  }

  function handleClose(): void {
    // Fires for Escape (when allowed), the [data-close-modal] button handled
    // by the shell's delegated click handler, and backdrop clicks below.
    // Parent-driven closes unmount the dialog instead and skip this handler.
    open = false;
    onClose();
  }

  function handleDialogClick(event: MouseEvent): void {
    // Backdrop clicks target the <dialog> element itself. stopPropagation
    // keeps the shell's delegated dialog click (which closes unconditionally)
    // from bypassing preventClose.
    if (event.target !== dialogEl) return;
    event.stopPropagation();
    if (!preventClose) {
      dialogEl?.close();
    }
  }
</script>

{#if open}
  <dialog
    class="modal {wide ? 'modal-wide' : ''}"
    {role}
    aria-labelledby="modal-title"
    bind:this={dialogEl}
    data-prevent-close={preventClose || undefined}
    onclick={handleDialogClick}
    oncancel={handleCancel}
    onclose={handleClose}
  >
    <div class="modal-card {wide ? 'wide-modal' : ''} {className}">
      <div class="modal-heading">
        <div>
          {#if kicker}
            <span class="modal-kicker">{kicker}</span>
          {/if}
          <h2 class="modal-title" id="modal-title">{title}</h2>
        </div>
        {#if !preventClose}
          <Button class="modal-close" type="button" data-close-modal aria-label={closeLabel}>×</Button>
        {/if}
      </div>
      {@render children?.()}
    </div>
  </dialog>
{/if}
