<script lang="ts">
  import { User } from '@lucide/svelte';
  import FlyingFishLogo from '../../components/FlyingFishLogo.svelte';
  import type { Locale } from '../../lib/i18n';
  import { formatTokenCount } from '../../lib/format';
  import type { Translate } from '../../lib/format';
  import type { ChatConversationMessage } from '../../lib/types';
  import { renderMarkdown, countCodeBlocks } from './markdown';
  import ChatTurnActions from './ChatTurnActions.svelte';

  export let tr: Translate;
  export let locale: Locale;
  export let messages: ChatConversationMessage[] = [];
  export let streamingId: string | null = null;
  export let onQuote: (text: string, index: number) => void = () => undefined;
  export let onEdit: (index: number) => void = () => undefined;
  export let onRegenerate: () => void = () => undefined;
  export let onContinue: () => void = () => undefined;
  export let onCopied: () => void = () => undefined;

  let threadElement: HTMLElement | null = null;
  let stickToBottom = true;
  /** Id of the code block whose text is captured for copying. */
  let copiedBlockId: string | null = null;

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

  $: lastIndex = messages.length - 1;

  const rendered = new Map<string, string>();

  function markdownFor(message: ChatConversationMessage): string {
    const cached = rendered.get(message.id);
    if (cached !== undefined) return cached;
    const html = renderMarkdown(message.text);
    if (rendered.size > 200) rendered.clear();
    rendered.set(message.id, html);
    return html;
  }

  function invalidate(message: ChatConversationMessage): void {
    if (rendered.has(message.id)) {
      rendered.set(message.id, renderMarkdown(message.text));
    }
  }

  $: if (streamingId) {
    const streaming = messages.find((message) => message.id === streamingId);
    if (streaming) invalidate(streaming);
  }

  function codeBlockText(message: ChatConversationMessage, position: number): string {
    const blocks = message.text.match(/```[\w]*\n?([\s\S]*?)```/g) ?? [];
    const block = blocks[position] ?? '';
    return block.replace(/^```[\w]*\n?/, '').replace(/```$/, '');
  }

  async function copyCodeBlock(message: ChatConversationMessage, position: number): Promise<void> {
    const text = codeBlockText(message, position);
    try {
      await navigator.clipboard?.writeText(text);
      copiedBlockId = `${message.id}:${position}`;
      window.setTimeout(() => {
        if (copiedBlockId === `${message.id}:${position}`) copiedBlockId = null;
      }, 1_500);
    } catch {
      // Clipboard unavailable (permissions, non-secure context); no banner.
    }
    onCopied();
  }

  function codeBlocks(message: ChatConversationMessage): number {
    return countCodeBlocks(message.text);
  }
</script>

<div class="chat-conversation" aria-live="polite">
  {#if !messages.length}
    <slot name="empty" />
  {:else}
    <div class="chat-thread" bind:this={threadElement} on:scroll={handleScroll}>
      {#each messages as message, index (message.id)}
        <article
          class="chat-turn"
          class:assistant={message.role === 'assistant'}
          class:error={Boolean(message.error)}
          class:streaming={message.id === streamingId}
        >
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
              {#if message.edited}
                <span class="chat-edited">{tr('edited')}</span>
              {/if}
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
              {#if message.role === 'assistant'}
                {@const html = markdownFor(message)}
                <div class="chat-turn-text chat-turn-markdown">{@html html}</div>
                {#if codeBlocks(message)}
                  <div class="chat-code-actions">
                    {#each Array(codeBlocks(message)) as _, position}
                      <button
                        class="chat-code-copy"
                        type="button"
                        on:click={() => copyCodeBlock(message, position)}
                      >
                        {copiedBlockId === `${message.id}:${position}` ? tr('Copied') : `${tr('Copy code')} ${position + 1}`}
                      </button>
                    {/each}
                  </div>
                {/if}
              {:else}
                <p class="chat-turn-text">{message.text}</p>
              {/if}
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
              <ChatTurnActions
                {tr}
                {message}
                {index}
                {lastIndex}
                onQuote={onQuote}
                onEdit={onEdit}
                onRegenerate={onRegenerate}
                onContinue={onContinue}
                onCopy={() => onCopied()}
              />
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

  /* Action row belongs to the child, but the hover scope lives here. */
  .chat-turn :global(.turn-actions) {
    opacity: 1;
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

  .chat-edited {
    font-size: 10px;
    font-style: italic;
    color: var(--muted);
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

  /* Markdown rendering: block flow inside the bubble, pre-wrap only for
     plain user turns. */
  .chat-turn-markdown {
    white-space: normal;
  }

  .chat-turn-markdown :global(.md-p) {
    margin: 0 0 8px;
  }

  .chat-turn-markdown :global(.md-p:last-child) {
    margin-bottom: 0;
  }

  .chat-turn-markdown :global(.md-h) {
    margin: 10px 0 6px;
    font-weight: var(--font-bold);
    color: var(--ink);
  }

  .chat-turn-markdown :global(.md-h1) { font-size: var(--text-base); }
  .chat-turn-markdown :global(.md-h2) { font-size: var(--text-sm); }
  .chat-turn-markdown :global(.md-h3) { font-size: var(--text-sm); }

  .chat-turn-markdown :global(.md-list) {
    margin: 0 0 8px;
    padding-left: 20px;
  }

  .chat-turn-markdown :global(.md-list li) {
    margin: 2px 0;
  }

  .chat-turn-markdown :global(.md-quote) {
    margin: 0 0 8px;
    padding: 6px 10px;
    border-left: 3px solid var(--ink);
    background: rgba(0, 0, 0, 0.04);
    border-radius: 6px;
  }

  .chat-turn-markdown :global(.md-code) {
    padding: 1px 5px;
    border: 1px solid rgba(0, 0, 0, 0.25);
    border-radius: 5px;
    background: rgba(0, 0, 0, 0.05);
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 0.92em;
  }

  .chat-turn-markdown :global(.md-code-block) {
    margin: 8px 0;
    border: 1.5px solid var(--ink);
    border-radius: 8px;
    background: #0f172a;
    overflow: hidden;
  }

  .chat-turn-markdown :global(.md-code-lang) {
    display: inline-block;
    padding: 3px 8px;
    font-size: 10px;
    color: #94a3b8;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .chat-turn-markdown :global(.md-pre) {
    margin: 0;
    padding: 10px 12px;
    overflow-x: auto;
  }

  .chat-turn-markdown :global(.md-pre code) {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    line-height: 1.5;
    color: #e2e8f0;
    white-space: pre;
  }

  .chat-turn-markdown :global(.md-link) {
    color: #c2410c;
    text-decoration: underline;
  }

  .chat-turn-markdown :global(.md-strong) { font-weight: var(--font-bold); }
  .chat-turn-markdown :global(.md-em) { font-style: italic; }

  .chat-turn.error .chat-turn-text {
    background: #fef2f2;
  }

  .chat-turn.streaming .chat-turn-text::after {
    content: '▍';
    display: inline-block;
    margin-left: 2px;
    animation: chat-caret 1s steps(2) infinite;
  }

  @keyframes chat-caret {
    0%, 49% { opacity: 1; }
    50%, 100% { opacity: 0; }
  }

  .chat-code-actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .chat-code-copy {
    padding: 3px 8px;
    border: 1.5px solid var(--ink);
    border-radius: 8px;
    background: var(--paper);
    color: var(--ink);
    font-size: 10px;
    cursor: pointer;
    box-shadow: 1.5px 1.5px 0 var(--ink);
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
  :global(:root[data-theme='dark']) .chat-turn-markdown :global(.md-quote) { background: rgba(255, 255, 255, 0.05); }
  :global(:root[data-theme='dark']) .chat-turn-markdown :global(.md-code) {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.2);
  }
  :global(:root[data-theme='dark']) .chat-turn-markdown :global(.md-link) { color: #fdba74; }
  :global(:root[data-theme='dark']) .chat-code-copy { background: rgba(255, 255, 255, 0.04); color: #c9cdde; }
</style>
