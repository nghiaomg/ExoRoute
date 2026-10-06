<script lang="ts">
  import { FileText, Image as ImageIcon, Paperclip, SendHorizontal, Settings2, Square, X } from '@lucide/svelte';
  import { afterUpdate } from 'svelte';
  import ArkField from '../../components/ArkField.svelte';
  import ArkSelect from '../../components/ArkSelect.svelte';
  import type { Translate } from '../../lib/format';
  import type { ChatAttachmentInput, ChatThinkingMode } from '../../lib/types';
  import {
    IMAGE_MEDIA_TYPES,
    DOCUMENT_MEDIA_TYPES,
    MAX_ATTACHMENT_BYTES,
    MAX_CHAT_ATTACHMENTS,
    THINKING_MODES,
    totalAttachmentBytes,
    type ChatDraft,
  } from './chat.state';

  export let tr: Translate;
  export let disabled = false;
  /** The page owns the draft; the composer reports edits through onDraftChange. */
  export let draft: ChatDraft;
  export let onDraftChange: (draft: ChatDraft) => void;
  export let onSend: (draft: ChatDraft) => void;
  /** True while a turn is in flight: the send affordance becomes Stop. */
  export let sending = false;
  export let onStop: () => void = () => {};

  let attachmentError = '';
  let fileInput: HTMLInputElement | null = null;
  let messageInput: HTMLTextAreaElement | null = null;
  let settingsOpen = false;

  $: attachmentTotalBytes = totalAttachmentBytes(draft.attachments);
  $: canSend = !disabled && draft.message.trim().length > 0;
  $: thinkingItems = THINKING_MODES.map((mode) => ({
    label: tr(mode.labelKey),
    value: mode.value,
  }));

  function updateDraft(patch: Partial<ChatDraft>): void {
    onDraftChange({ ...draft, ...patch });
  }

  function acceptAttribute(): string {
    return [...IMAGE_MEDIA_TYPES, ...DOCUMENT_MEDIA_TYPES].join(',');
  }

  async function handleFiles(event: Event): Promise<void> {
    attachmentError = '';
    const input = event.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = '';
    if (!files.length) return;
    const nextAttachments: ChatAttachmentInput[] = [];
    for (const file of files) {
      if (draft.attachments.length + nextAttachments.length >= MAX_CHAT_ATTACHMENTS) {
        attachmentError = tr('Too many attachments.');
        break;
      }
      if (file.size > MAX_ATTACHMENT_BYTES) {
        attachmentError = tr('Attachment is too large.');
        continue;
      }
      const isImage = (IMAGE_MEDIA_TYPES as readonly string[]).includes(file.type);
      const isDocument = (DOCUMENT_MEDIA_TYPES as readonly string[]).includes(file.type);
      if (!isImage && !isDocument) {
        attachmentError = tr('Only images (JPEG, PNG, GIF, WebP), PDFs, and text files are supported.');
        continue;
      }
      const data = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onerror = () => reject(new Error('read failed'));
        reader.onload = () => {
          const result = String(reader.result ?? '');
          // Strip the data URL prefix; the relay expects raw base64.
          resolve(result.includes(',') ? result.slice(result.indexOf(',') + 1) : result);
        };
        reader.readAsDataURL(file);
      }).catch(() => '');
      if (!data) {
        attachmentError = tr('Attachment could not be read.');
        continue;
      }
      nextAttachments.push({
        kind: isImage ? 'image' : 'document',
        media_type: file.type || (isImage ? 'image/png' : 'application/pdf'),
        data,
        filename: file.name,
      });
    }
    if (nextAttachments.length) {
      updateDraft({ attachments: [...draft.attachments, ...nextAttachments] });
    }
  }

  function removeAttachment(index: number): void {
    updateDraft({
      attachments: draft.attachments.filter((_, position) => position !== index),
    });
  }

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    if (!canSend) return;
    onSend(draft);
  }

  /** Autosizes the input to its content, capped by the CSS max height. */
  function resizeInput(): void {
    if (!messageInput) return;
    messageInput.style.height = 'auto';
    messageInput.style.height = `${Math.min(messageInput.scrollHeight, 160)}px`;
  }

  // afterUpdate runs once the DOM already reflects the draft, so measuring here
  // needs no tick(). Awaiting tick() inside a `$:` statement kept the legacy
  // pre-effect dirty and could flush forever while the page waited on a pending
  // request, which starved timers and froze the chat page.
  afterUpdate(resizeInput);

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      if (canSend) onSend(draft);
      return;
    }
    if (event.key === 'Escape' && settingsOpen) {
      settingsOpen = false;
    }
  }

  function toggleSettings(): void {
    settingsOpen = !settingsOpen;
  }

  /**
   * The model picker lives inside the composer form, so Enter there would
   * implicitly submit the draft. Enter only ever selects a model; sending
   * stays a decision made in the message field or on the send button.
   */
  function blockSubmitEnter(event: KeyboardEvent): void {
    if (event.key === 'Enter') event.preventDefault();
  }
</script>

<form class="chat-composer" on:submit|preventDefault={submit}>
  <textarea
    bind:this={messageInput}
    class="chat-message-input"
    value={draft.message}
    on:input={(event) => updateDraft({ message: event.currentTarget.value })}
    placeholder={tr('Type a message…')}
    {disabled}
    rows={1}
    on:keydown={handleKeydown}
  ></textarea>

  {#if settingsOpen}
    <div class="chat-composer-settings">
      <ArkField
        label={tr('System prompt (optional)')}
        value={draft.systemPrompt}
        maxlength={32000}
        placeholder={tr('Instructions the model follows for every message')}
        {disabled}
        onValueChange={(value) => updateDraft({ systemPrompt: value })}
      />
      <ArkSelect
        label={tr('Thinking')}
        items={thinkingItems}
        value={draft.thinkingMode}
        {disabled}
        onValueChange={(value) => updateDraft({ thinkingMode: value as ChatThinkingMode })}
      />
      {#if draft.thinkingMode === 'override'}
        <ArkField
          label={tr('Thinking override text')}
          value={draft.thinkingOverride}
          maxlength={4096}
          placeholder={tr('Replacement text for upstream thinking blocks')}
          {disabled}
          onValueChange={(value) => updateDraft({ thinkingOverride: value })}
        />
      {/if}
      <ArkField
        label={tr('Temperature')}
        value={draft.temperature}
        placeholder={tr('Leave empty to use the provider default.')}
        {disabled}
        onValueChange={(value) => updateDraft({ temperature: value })}
      />
      <ArkField
        label={tr('Top P')}
        value={draft.topP}
        placeholder={tr('Leave empty to use the provider default.')}
        {disabled}
        onValueChange={(value) => updateDraft({ topP: value })}
      />
      <ArkField
        label={tr('Max tokens')}
        value={draft.maxTokens}
        placeholder={tr('Leave empty to use the provider default.')}
        {disabled}
        onValueChange={(value) => updateDraft({ maxTokens: value })}
      />
    </div>
  {/if}

  {#if draft.attachments.length}
    <ul class="chat-attachment-list">
      {#each draft.attachments as attachment, index (attachment.filename ?? index)}
        <li>
          {#if attachment.kind === 'image'}<ImageIcon size={12} />{:else}<FileText size={12} />{/if}
          <span class="chat-attachment-name">{attachment.filename ?? attachment.media_type}</span>
          <button class="chat-attachment-remove" type="button" aria-label={tr('Remove attachment')} on:click={() => removeAttachment(index)}>
            <X size={12} />
          </button>
        </li>
      {/each}
      <li class="chat-attachment-total">{attachmentTotalBytes} B</li>
    </ul>
  {/if}
  {#if attachmentError}<div class="chat-attachment-error" role="alert">{attachmentError}</div>{/if}

  <div class="chat-composer-bar">
    <div class="chat-composer-tools">
      <input
        class="chat-file-input"
        type="file"
        accept={acceptAttribute()}
        multiple
        hidden
        bind:this={fileInput}
        on:change={handleFiles}
      />
      <button class="chat-tool-button" type="button" {disabled} title={tr('Attach')} on:click={() => fileInput?.click()}>
        <Paperclip size={14} />
      </button>
      <button
        class="chat-tool-button"
        class:active={settingsOpen}
        type="button"
        {disabled}
        title={tr('Chat settings')}
        on:click={toggleSettings}
      >
        <Settings2 size={14} />
      </button>
    </div>
    <!-- Layout-only wrapper: it exists to intercept Enter before the form can
         treat it as an implicit submit. -->
    <div class="chat-composer-model" role="presentation" on:keydown={blockSubmitEnter}>
      <slot name="model-picker" />
    </div>
    {#if sending}
      <button class="chat-stop-button" type="button" on:click={onStop} title={tr('Stop')} aria-label={tr('Stop')}>
        <Square size={14} />
      </button>
    {:else}
      <button class="chat-send-button" type="submit" disabled={!canSend} title={tr('Send')} aria-label={tr('Send')}>
        <SendHorizontal size={15} />
      </button>
    {/if}
  </div>
  <p class="chat-hint">{tr('Enter to send, Shift+Enter for a new line. Attach up to {count} images or documents.', { count: MAX_CHAT_ATTACHMENTS })}</p>
</form>

<style>
  /* Soft Neo-Brutalism: 2px ink borders, hard offset ink shadows, soft
     radii, focus swaps the hard shadow for the brand accent. */
  .chat-composer {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border: 2px solid var(--ink);
    border-radius: 14px;
    box-shadow: 4px 4px 0 var(--ink);
    background: var(--paper);
    transition: border-color 0.14s ease, box-shadow 0.14s ease;
  }

  .chat-composer:focus-within {
    border-color: #f97316;
    box-shadow: 4px 4px 0 #f97316;
  }

  .chat-message-input {
    width: 100%;
    min-height: 38px;
    max-height: 160px;
    padding: 8px 10px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
    resize: none;
    outline: none;
  }

  .chat-message-input::placeholder {
    color: var(--muted);
  }

  .chat-message-input:disabled {
    opacity: 0.55;
  }

  .chat-composer-settings {
    display: grid;
    gap: 10px;
    padding: 10px;
    border: 2px dashed var(--ink);
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.4);
  }

  .chat-attachment-list {
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    font-size: var(--text-2xs);
  }

  .chat-attachment-list li {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 8px;
    border: 2px solid var(--ink);
    border-radius: var(--radius-full);
    background: #fff7ed;
    font-weight: var(--font-semibold);
  }

  .chat-attachment-total {
    border: none;
    color: var(--muted);
  }

  .chat-attachment-name {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chat-attachment-remove {
    display: inline-flex;
    border: 0;
    background: transparent;
    padding: 0;
    color: var(--ink);
    cursor: pointer;
  }

  .chat-attachment-remove:hover {
    color: var(--ink);
  }

  .chat-attachment-error {
    color: #b91c1c;
    font-size: var(--text-2xs);
    font-weight: var(--font-semibold);
  }

  .chat-composer-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 6px;
    border-top: 2px solid var(--ink);
  }

  .chat-composer-tools {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .chat-composer-model {
    display: flex;
    flex: 1 1 auto;
    align-items: center;
    justify-content: flex-end;
    min-width: 0;
  }

  .chat-tool-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 2px solid var(--ink);
    border-radius: 9px;
    background: var(--paper);
    box-shadow: 2px 2px 0 var(--ink);
    color: var(--ink);
    cursor: pointer;
    transition: transform 0.12s ease, box-shadow 0.12s ease, background 0.12s ease;
  }

  .chat-tool-button:hover:not(:disabled) {
    transform: translate(-1px, -1px);
    box-shadow: 3px 3px 0 var(--ink);
  }

  .chat-tool-button.active {
    background: #ffedd5;
  }

  .chat-tool-button:active:not(:disabled) {
    transform: translate(1px, 1px);
    box-shadow: 1px 1px 0 var(--ink);
  }

  .chat-tool-button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
    box-shadow: none;
  }

  .chat-send-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 2px solid var(--ink);
    border-radius: 10px;
    background: #f97316;
    box-shadow: 2px 2px 0 var(--ink);
    color: #fff;
    cursor: pointer;
    transition: transform 0.12s ease, box-shadow 0.12s ease;
  }

  .chat-send-button:hover:not(:disabled) {
    transform: translate(-1px, -1px);
    box-shadow: 3px 3px 0 var(--ink);
  }

  .chat-send-button:active:not(:disabled) {
    transform: translate(1px, 1px);
    box-shadow: 1px 1px 0 var(--ink);
  }

  .chat-send-button:disabled {
    background: rgba(255, 255, 255, 0.55);
    color: var(--muted);
    cursor: not-allowed;
    box-shadow: none;
  }

  .chat-stop-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 2px solid var(--ink);
    border-radius: 10px;
    background: #fee2e2;
    box-shadow: 2px 2px 0 var(--ink);
    color: #b91c1c;
    cursor: pointer;
    transition: transform 0.12s ease, box-shadow 0.12s ease;
  }

  .chat-stop-button:hover {
    transform: translate(-1px, -1px);
    box-shadow: 3px 3px 0 var(--ink);
  }

  .chat-stop-button:active {
    transform: translate(1px, 1px);
    box-shadow: 1px 1px 0 var(--ink);
  }

  .chat-hint {
    margin: 0;
    padding: 0 2px;
    font-size: 11px;
    color: var(--muted);
  }

  .chat-file-input {
    display: none;
  }

  :global(:root[data-theme='dark']) .chat-composer-settings { background: rgba(255, 255, 255, 0.03); }
  :global(:root[data-theme='dark']) .chat-tool-button { background: rgba(255, 255, 255, 0.04); }
  :global(:root[data-theme='dark']) .chat-tool-button.active { background: rgba(249, 115, 22, 0.16); }
  :global(:root[data-theme='dark']) .chat-send-button:disabled { background: rgba(255, 255, 255, 0.06); }
  :global(:root[data-theme='dark']) .chat-stop-button { background: rgba(248, 113, 113, 0.16); color: #fecaca; }
  :global(:root[data-theme='dark']) .chat-attachment-list li { background: rgba(249, 115, 22, 0.1); }
  :global(:root[data-theme='dark']) .chat-attachment-error { color: #fecaca; }
</style>
