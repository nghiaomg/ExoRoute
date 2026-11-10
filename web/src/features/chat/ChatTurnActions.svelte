<script lang="ts">
  import { Copy, Pencil, Quote, RefreshCw, ArrowDownToLine } from '@lucide/svelte';
  import Button from '../../components/Button.svelte';
  import type { Translate } from '../../lib/format';
  import type { ChatConversationMessage } from '../../lib/types';

  export let tr: Translate;
  export let message: ChatConversationMessage;
  export let index: number;
  /** Index of the last turn; regenerate/continue only apply there. */
  export let lastIndex: number;
  /** Populates the composer with `> text` for a follow-up question. */
  export let onQuote: (text: string, index: number) => void;
  /** Enters edit mode for this user turn. */
  export let onEdit: (index: number) => void;
  /** Re-asks the turn before the last assistant reply. */
  export let onRegenerate: () => void;
  /** Asks the model to continue the last assistant reply. */
  export let onContinue: () => void;
  /** Registers this turn's text for the thread-wide export actions. */
  export let onCopy: (text: string) => void;

  const isUser = message.role === 'user' && !message.error;
  const isAssistant = message.role === 'assistant' && !message.error;

  function copyTurn(): void {
    void navigator.clipboard?.writeText(message.text).catch(() => undefined);
    onCopy(message.text);
  }

  function quoteTurn(): void {
    onQuote(message.text, index);
  }
</script>

<div class="turn-actions">
  <Button class="turn-action" type="button" title={tr('Copy')} onclick={copyTurn}>
    <Copy size={12} />
  </Button>
  {#if !message.error}
    <Button class="turn-action" type="button" title={tr('Quote into composer')} onclick={quoteTurn}>
      <Quote size={12} />
    </Button>
  {/if}
  {#if isUser}
    <Button class="turn-action" type="button" title={tr('Edit and resend')} onclick={() => onEdit(index)}>
      <Pencil size={12} />
    </Button>
  {/if}
  {#if isAssistant && index === lastIndex}
    <Button class="turn-action" type="button" title={tr('Regenerate')} onclick={onRegenerate}>
      <RefreshCw size={12} />
    </Button>
  {/if}
  {#if isAssistant && index === lastIndex}
    <Button class="turn-action" type="button" title={tr('Continue generation')} onclick={onContinue}>
      <ArrowDownToLine size={12} />
    </Button>
  {/if}
</div>

<style>
  :global(.turn-actions) {
    display: flex;
    gap: 4px;
    opacity: 0;
    transition: opacity 0.12s ease;
  }

  /* Hover reveal is owned by the parent (it renders .chat-turn). */
  .turn-actions:focus-within {
    opacity: 1;
  }

  :global(.turn-action) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: var(--nb-border);
    border-radius: 8px;
    background: var(--paper);
    color: var(--ink);
    cursor: pointer;
    box-shadow: var(--nb-shadow-sm);
    transition: transform 0.1s ease, box-shadow 0.1s ease;
  }

  :global(.turn-action:hover) {
    transform: translate(-1px, -1px);
    box-shadow: var(--nb-shadow-sm);
  }

  :global(.turn-action:active) {
    transform: translate(1px, 1px);
    box-shadow: var(--nb-shadow-sm);
  }

  :global(:root[data-theme='dark'] .turn-action) {
    background: rgba(255, 255, 255, 0.04);
    color: #c9cdde;
  }
</style>
