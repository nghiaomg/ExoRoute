<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Dialog } from '@ark-ui/svelte/dialog';
  import { Portal } from '@ark-ui/svelte/portal';
  import { X } from '@lucide/svelte';

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

  function handleOpenChange(details: { open: boolean }): void {
    if (!details.open && !preventClose) {
      onClose();
    }
  }
</script>

{#if open}
  <Dialog.Root
    bind:open
    role={role}
    lazyMount
    unmountOnExit
    onOpenChange={handleOpenChange}
    closeOnInteractOutside={!preventClose}
    closeOnEscape={!preventClose}
    trapFocus
  >
    <Portal>
      <Dialog.Backdrop class="modal-backdrop" />
      <Dialog.Positioner class="modal-positioner">
        <Dialog.Content class="modal-card {wide ? 'wide-modal' : ''} {className}">
          <div class="modal-header">
            <div>
              {#if kicker}
                <span class="modal-kicker">{kicker}</span>
              {/if}
              <Dialog.Title class="modal-title" id="modal-title">{title}</Dialog.Title>
            </div>
            {#if !preventClose}
              <Dialog.CloseTrigger type="button" class="icon-button" aria-label={closeLabel}>
                <X size={18} />
              </Dialog.CloseTrigger>
            {/if}
          </div>
          {@render children?.()}
        </Dialog.Content>
      </Dialog.Positioner>
    </Portal>
  </Dialog.Root>
{/if}
