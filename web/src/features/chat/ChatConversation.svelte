<script lang="ts">
  import { User } from '@lucide/svelte';
  import FlyingFishLogo from '../../components/FlyingFishLogo.svelte';
  import type { Locale } from '../../lib/i18n';
  import { formatTokenCount } from '../../lib/format';
  import type { Translate } from '../../lib/format';
  import type { ChatConversationMessage } from '../../lib/types';

  export let tr: Translate;
  export let locale: Locale;
  export let messages: ChatConversationMessage[] = [];

  let threadElement: HTMLElement | null = null;
  let stickToBottom = true;

  // Keep the newest turn in view while the thread grows, but stop fighting the
  // user the moment they scroll up to read history.
  function handleScroll(): void {
    if (!threadElement) return;
    stickToBottom =
      threadElement.scrollHeight - threadElement.scrollTop - threadElement.clientHeight < 40;
  }

  $: if (stickToBottom && threadElement && messages.length) {
    threadElement.scrollTop = threadElement.scrollHeight;
  }
</script>

<div class="chat-conversation" aria-live="polite">
  {#if !messages.length}
    <slot name="empty" />
  {:else}
    <div class="chat-thread" bind:this={threadElement} on:scroll={handleScroll}>
      {#each messages as message (message.id)}
        <article class="chat-turn" class:assistant={message.role === 'assistant'} class:error={Boolean(message.error)}>
          <div class="chat-turn-avatar">
            {#if message.role === 'user'}
              <span class="chat-avatar user"><User size={13} /></span>
            {:else}
              <span class="chat-avatar assistant"><FlyingFishLogo size={13} variant="mark" /></span>
            {/if}
          </div>
          <div class="chat-turn-main">
            <header class="chat-turn-header">
              <strong>{message.role === 'user' ? tr('You') : message.model || tr('Assistant')}</strong>
              {#if message.attachmentCount}
                <span class="chat-attachment-count">{tr('{count} attachments', { count: message.attachmentCount })}</span>
              {/if}
            </header>
            {#if message.error}
              <p class="chat-turn-error">{message.error}</p>
            {:else}
              {#if message.reasoning}
                <details class="chat-reasoning">
                  <summary>{tr('Reasoning')}</summary>
                  <p>{message.reasoning}</p>
                </details>
              {/if}
              <p class="chat-turn-text">{message.text}</p>
              {#if message.role === 'assistant' && (message.usage || message.duration_ms != null)}
                <footer class="chat-turn-meta">
                  {#if message.usage}
                    <span>{tr('{input} in / {output} out', { input: formatTokenCount(message.usage.input_tokens, locale), output: formatTokenCount(message.usage.output_tokens, locale) })}</span>
                  {/if}
                  {#if message.duration_ms != null}
                    <span>{message.duration_ms} ms</span>
                  {/if}
                </footer>
              {/if}
            {/if}
          </div>
        </article>
      {/each}
    </div>
  {/if}
</div>

<style>
  .chat-conversation {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .chat-thread {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 4px 0 12px;
  }

  .chat-turn {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }

  .chat-turn.assistant {
    flex-direction: row-reverse;
  }

  .chat-turn-avatar {
    flex: 0 0 auto;
    padding-top: 2px;
  }

  .chat-turn-main {
    min-width: 0;
    max-width: 82%;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .chat-turn:not(.assistant) .chat-turn-main {
    align-items: flex-start;
  }

  .chat-turn.assistant .chat-turn-main {
    align-items: flex-end;
  }

  /* Soft Neo-Brutalism bubbles: 2px ink borders, hard offset shadows. */
  .chat-avatar {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: 2px solid var(--ink);
    border-radius: 50%;
    background: var(--paper);
    color: var(--ink);
    box-shadow: 2px 2px 0 var(--ink);
  }

  .chat-avatar.assistant {
    background: #ffedd5;
    color: #c2410c;
  }

  .chat-turn-header {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: var(--muted);
  }

  .chat-turn-header strong {
    font-size: var(--text-2xs);
    font-weight: var(--font-semibold);
    color: var(--ink);
  }

  .chat-attachment-count {
    font-size: 10px;
  }

  .chat-turn-text {
    margin: 0;
    padding: 9px 12px;
    border: 2px solid var(--ink);
    border-radius: 12px;
    box-shadow: 3px 3px 0 var(--ink);
    background: var(--paper);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: var(--text-sm);
    color: var(--ink);
  }

  .chat-turn.assistant .chat-turn-text {
    background: #ffedd5;
  }

  .chat-turn.error .chat-turn-text {
    background: #fef2f2;
  }

  .chat-turn-error {
    margin: 0;
    padding: 9px 12px;
    border: 2px solid #b91c1c;
    border-radius: 12px;
    box-shadow: 3px 3px 0 #b91c1c;
    background: #fef2f2;
    color: #b91c1c;
    font-size: var(--text-sm);
    font-weight: var(--font-semibold);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .chat-reasoning {
    max-width: 100%;
    font-size: 11px;
    color: var(--muted);
  }

  .chat-reasoning summary {
    cursor: pointer;
  }

  .chat-reasoning p {
    margin: 6px 0 0;
    padding: 8px 10px;
    border: 2px dashed var(--ink);
    border-radius: 8px;
    white-space: pre-wrap;
  }

  .chat-turn-meta {
    display: flex;
    gap: 10px;
    font-size: 10px;
    color: var(--muted);
  }

  :global(:root[data-theme='dark']) .chat-avatar { background: rgba(255, 255, 255, 0.04); color: #c9cdde; }
  :global(:root[data-theme='dark']) .chat-avatar.assistant { background: rgba(249, 115, 22, 0.16); color: #fdba74; }
  :global(:root[data-theme='dark']) .chat-turn-text { background: rgba(255, 255, 255, 0.03); }
  :global(:root[data-theme='dark']) .chat-turn.assistant .chat-turn-text {
    background: rgba(249, 115, 22, 0.12);
  }
  :global(:root[data-theme='dark']) .chat-turn.error .chat-turn-text { background: #2b1818; }
  :global(:root[data-theme='dark']) .chat-turn-error { border-color: #f87171; box-shadow: 3px 3px 0 #f87171; background: #2b1818; color: #fecaca; }
</style>
