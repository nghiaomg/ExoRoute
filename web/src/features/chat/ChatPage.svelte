<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Settings2, Trash2 } from '@lucide/svelte';
  import ChatConversation from './ChatConversation.svelte';
  import ChatComposer from './ChatComposer.svelte';
  import ChatModelPicker from './ChatModelPicker.svelte';
  import ShieldInfo from './ShieldInfo.svelte';
  import GatewayError from '../../components/GatewayError.svelte';
  import InlineLoading from '../../components/InlineLoading.svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import type { ChatConversationMessage, WorkspaceChatModelOption } from '../../lib/types';
  import {
    emptyDraft,
    errorMessage as buildErrorMessage,
    assistantMessageFromResult,
    userMessageFromDraft,
    validateDraft,
    type ChatDraft,
  } from './chat.state';

  export let tr: Translate;
  export let locale: Locale;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;

  let loadingModels = true;
  let modelOptions: WorkspaceChatModelOption[] = [];
  let modelsTruncated = false;
  let loadError = '';
  let generation = 0;

  let draft: ChatDraft = emptyDraft();
  let conversation: ChatConversationMessage[] = [];
  let sending = false;
  let sendError = '';

  $: suggestions = [
    { icon: 'wand' as const, title: tr('Draft a status update'), body: tr('Summarize this week\u2019s changes into three bullet points for the team.') },
    { icon: 'globe' as const, title: tr('Explain a concept'), body: tr('Explain how an AI gateway differs from a plain reverse proxy.') },
    { icon: 'code' as const, title: tr('Review a config'), body: tr('Write a hardened nginx TLS reverse-proxy snippet for a local gateway.') },
    { icon: 'sparkles' as const, title: tr('Brainstorm names'), body: tr('Suggest five product names for a local-first model router.') },
  ];

  async function loadModels(): Promise<void> {
    const requestGeneration = ++generation;
    loadingModels = true;
    loadError = '';
    onConnectionChange('loading');
    try {
      const result = await api.chatModels();
      if (requestGeneration !== generation) return;
      modelOptions = result.models;
      modelsTruncated = result.truncated;
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation) return;
      loadError = localizedError(error, 'Could not load the model list.', tr);
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) loadingModels = false;
    }
  }

  function resolveProviderId(modelId: string): string {
    return modelOptions.find((option) => option.id === modelId)?.provider_id ?? '';
  }

  async function handleSend(nextDraft: ChatDraft): Promise<void> {
    if (sending) return;
    sendError = '';
    const validated = validateDraft({ ...nextDraft, providerId: resolveProviderId(nextDraft.modelId) });
    if (!validated.ok) {
      sendError = tr(validated.errorKey);
      return;
    }
    sending = true;
    const userMessage = userMessageFromDraft(nextDraft);
    conversation = [...conversation, userMessage];
    try {
      const result = await api.chat(validated.input);
      conversation = [...conversation, assistantMessageFromResult(result)];
      // Clear only the consumed turn bubbles; settings and attachments stay.
      draft = { ...nextDraft, message: '', attachments: [] };
    } catch (error) {
      const detail = localizedError(error, 'The chat request failed.', tr);
      conversation = [...conversation, buildErrorMessage(detail)];
      sendError = detail;
    } finally {
      sending = false;
    }
  }

  function applySuggestion(body: string): void {
    draft = { ...draft, message: body };
  }

  function handleClear(): void {
    conversation = [];
    sendError = '';
  }

  onMount(() => {
    void loadModels();
  });
  onDestroy(() => {
    generation += 1;
  });
</script>

<div class="chat-workspace">
  <header class="chat-topbar">
    <div class="chat-topbar-leading">
      {#if conversation.length}
        <button class="chat-icon-action" type="button" onclick={handleClear} disabled={sending} title={tr('Clear conversation')}>
          <Trash2 size={14} />
        </button>
      {/if}
    </div>
    <div class="chat-topbar-trailing">
      {#if modelsTruncated}
        <span class="chat-topbar-note" role="status">{tr('Model list truncated')}</span>
      {/if}
      {#if !loadingModels && !loadError}
        <ChatModelPicker
          {tr}
          options={modelOptions}
          disabled={sending}
          modelId={draft.modelId}
          onModelIdChange={(modelId) => (draft = { ...draft, modelId })}
        />
      {/if}
      {#if conversation.length}
        <button
          class="chat-icon-action"
          type="button"
          onclick={handleClear}
          disabled={sending || !conversation.length}
          title={tr('Clear conversation')}
        >
          <Settings2 size={14} />
        </button>
      {/if}
    </div>
  </header>

  {#if loadError}
    <div class="chat-workspace-body">
      <GatewayError message={loadError} {tr} onRetry={loadModels} />
    </div>
  {:else if loadingModels}
    <div class="chat-workspace-body">
      <InlineLoading label={'Loading {page}…'} {tr} vars={{ page: tr('Chat').toLowerCase() }} />
    </div>
  {:else}
    <div class="chat-workspace-body">
      <ChatConversation {tr} {locale} messages={conversation}>
        <div slot="empty" class="chat-hero">
          <ShieldInfo {tr} />
          <h2 class="chat-hero-title">{tr('Workspace chat')}</h2>
          <p class="chat-hero-subtitle">{tr('Talk to one saved model. Nothing here is written to the database.')}</p>
          <div class="chat-hero-suggestions">
            {#each suggestions as suggestion (suggestion.title)}
              <button class="chat-suggestion" type="button" onclick={() => applySuggestion(suggestion.body)}>
                <span class="chat-suggestion-title">{suggestion.title}</span>
                <span class="chat-suggestion-body">{suggestion.body}</span>
              </button>
            {/each}
          </div>
        </div>
      </ChatConversation>
    </div>
    <div class="chat-workspace-dock">
      <ChatComposer
        {tr}
        disabled={sending || !draft.modelId}
        {draft}
        onDraftChange={(next) => (draft = next)}
        onSend={handleSend}
      />
      {#if sendError}<div class="chat-send-error" role="alert">{sendError}</div>{/if}
    </div>
  {/if}
</div>

<style>
  /* Soft Neo-Brutalism: 2px ink borders, hard offset ink shadows, soft
     radii, lift-on-hover / press-on-active, brand-orange accent. */
  .chat-workspace {
    display: flex;
    flex-direction: column;
    height: calc(100vh - var(--app-header-height, 56px) - 24px);
    min-height: 480px;
    overflow: hidden;
    border: 2px solid var(--ink);
    border-radius: 16px;
    box-shadow: 5px 5px 0 var(--ink);
    background: var(--paper);
  }

  .chat-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex: 0 0 auto;
    padding: 8px 10px;
    border-bottom: 2px solid var(--ink);
    background: #fff7ed;
  }

  .chat-topbar-trailing {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .chat-topbar-leading {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .chat-topbar-note {
    font-size: 11px;
    color: var(--muted);
  }

  .chat-icon-action {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 2px solid var(--ink);
    border-radius: 10px;
    background: var(--paper);
    box-shadow: 2px 2px 0 var(--ink);
    color: var(--ink);
    cursor: pointer;
    transition: transform 0.12s ease, box-shadow 0.12s ease;
  }

  .chat-icon-action:hover:not(:disabled) {
    transform: translate(-1px, -1px);
    box-shadow: 3px 3px 0 var(--ink);
  }

  .chat-icon-action:active:not(:disabled) {
    transform: translate(1px, 1px);
    box-shadow: 1px 1px 0 var(--ink);
  }

  .chat-icon-action:disabled {
    opacity: 0.45;
    cursor: not-allowed;
    box-shadow: none;
  }

  .chat-workspace-body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 16px;
  }

  .chat-workspace-body > :global(*) {
    max-width: 760px;
    margin-left: auto;
    margin-right: auto;
  }

  .chat-workspace-dock {
    flex: 0 0 auto;
    padding: 10px 16px 14px;
  }

  .chat-workspace-dock > :global(*) {
    max-width: 760px;
    margin-left: auto;
    margin-right: auto;
  }

  .chat-hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 28px 0 12px;
    text-align: center;
  }

  .chat-hero :global(.chat-privacy-note) {
    text-align: left;
  }

  .chat-hero-title {
    margin: 0;
    font-size: var(--text-xl);
    font-weight: var(--font-extrabold);
    color: var(--ink);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .chat-hero-subtitle {
    margin: 0;
    max-width: 420px;
    font-size: var(--text-xs);
    color: var(--muted);
  }

  .chat-hero-suggestions {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    width: 100%;
    margin-top: 10px;
  }

  .chat-suggestion {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 12px;
    border: 2px solid var(--ink);
    border-radius: 12px;
    background: var(--paper);
    box-shadow: 3px 3px 0 var(--ink);
    text-align: left;
    cursor: pointer;
    transition: transform 0.12s ease, box-shadow 0.12s ease;
  }

  .chat-suggestion:hover {
    transform: translate(-1px, -1px);
    box-shadow: 4px 4px 0 var(--ink);
  }

  .chat-suggestion:active {
    transform: translate(2px, 2px);
    box-shadow: 1px 1px 0 var(--ink);
  }

  .chat-suggestion-title {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-xs);
    font-weight: var(--font-bold);
    color: var(--ink);
  }

  .chat-suggestion-body {
    font-size: 11px;
    color: var(--muted);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .chat-send-error {
    margin: 10px auto 0;
    max-width: 760px;
    padding: 9px 12px;
    border: 2px solid #b91c1c;
    border-radius: 12px;
    box-shadow: 3px 3px 0 #b91c1c;
    background: #fef2f2;
    color: #b91c1c;
    font-size: 12px;
    font-weight: var(--font-semibold);
  }

  :global(:root[data-theme='dark']) .chat-workspace { background: rgba(14, 16, 26, 0.72); }
  :global(:root[data-theme='dark']) .chat-topbar { background: rgba(249, 115, 22, 0.08); }
  :global(:root[data-theme='dark']) .chat-icon-action { background: rgba(255, 255, 255, 0.04); }
  :global(:root[data-theme='dark']) .chat-suggestion { background: rgba(255, 255, 255, 0.02); }
  :global(:root[data-theme='dark']) .chat-send-error {
    border-color: #f87171;
    box-shadow: 3px 3px 0 #f87171;
    background: #2b1818;
    color: #fecaca;
  }

  @media (max-width: 650px) {
    .chat-hero-suggestions {
      grid-template-columns: 1fr;
    }
  }
</style>
