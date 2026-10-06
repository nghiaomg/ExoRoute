<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    Columns2,
    FileDown,
    Braces,
    Library,
    Printer,
    SquareStack,
    Trash2,
    Command,
  } from '@lucide/svelte';
  import ChatConversation from './ChatConversation.svelte';
  import ChatComposer from './ChatComposer.svelte';
  import ChatModelPicker from './ChatModelPicker.svelte';
  import ChatSessionMeter from './ChatSessionMeter.svelte';
  import ChatHero from './ChatHero.svelte';
  import ChatNotice from './ChatNotice.svelte';
  import ChatComparePanel from './ChatComparePanel.svelte';
  import ChatPromptLibrary from './ChatPromptLibrary.svelte';
  import ChatCommandPalette from './ChatCommandPalette.svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
  import type {
    ChatConversationMessage,
    WorkspaceChatInput,
    WorkspaceChatModelOption,
    WorkspaceChatModelsResult,
  } from '../../lib/types';
  import {
    MAX_HISTORY_TURNS,
    MODEL_LIST_TIMEOUT_MS,
    buildChatHistory,
    validateDraft,
    userMessageFromDraft,
    defaultModelId,
    loadLastModelId,
    saveLastModelId,
    emptyDraft,
    errorMessage as buildErrorMessage,
    historyPressure,
    sessionTokens,
    streamingAssistantMessage,
    createDeltaBuffer,
    branchFromEdit,
    dropTrailingAssistantTurns,
    continueInstruction,
    contextBeforeFinalUserTurn,
    conversationToMarkdown,
    conversationToJson,
    conversationMarkdownFilename,
    conversationJsonFilename,
    downloadTextFile,
    printConversation,
    loadPriceTable,
    estimateSessionCost,
    loadPrompts,
    savePrompts,
    makePrompt,
    findPromptByName,
    parseSlashSave,
    parseSlashUse,
    raceWithDeadline,
    type DeadlineRace,
    type ChatDraft,
  } from './chat.state';
  import { markdownPreview } from './markdown';

  export let tr: Translate;
  export let locale: Locale;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;

  let loadingModels = true;
  let modelOptions: WorkspaceChatModelOption[] = [];
  let modelsTruncated = false;
  let loadError = '';
  let generation = 0;
  let modelLoad: DeadlineRace<WorkspaceChatModelsResult> | null = null;

  let draft: ChatDraft = emptyDraft();
  let conversation: ChatConversationMessage[] = [];
  let sending = false;
  let sendError = '';
  let sendNotice = '';
  let streamingId: string | null = null;
  let chatController: AbortController | null = null;

  let compareOpen = false;
  let compareInput: WorkspaceChatInput | null = null;
  let libraryOpen = false;
  let paletteOpen = false;
  /** When set, the composer edits this user turn and resends as a branch. */
  let editingIndex: number | null = null;
  let priceTable = loadPriceTable();

  // The thread lives in memory only; these helpers describe the bounded slice
  // the relay replays as context and the session token totals.
  $: historySlice = buildChatHistory(conversation);
  $: historySeverity = historyPressure(historySlice);
  $: tokens = sessionTokens(conversation);
  $: cost = estimateSessionCost(
    priceTable,
    draft.modelId,
    tokens.input,
    tokens.output,
  );

  $: suggestions = [
    { icon: 'wand' as const, title: tr('Draft a status update'), body: tr('Summarize this week\u2019s changes into three bullet points for the team.') },
    { icon: 'globe' as const, title: tr('Explain a concept'), body: tr('Explain how an AI gateway differs from a plain reverse proxy.') },
    { icon: 'code' as const, title: tr('Review a config'), body: tr('Write a hardened nginx TLS reverse-proxy snippet for a local gateway.') },
    { icon: 'sparkles' as const, title: tr('Brainstorm names'), body: tr('Suggest five product names for a local-first model router.') },
  ];

  async function loadModels(): Promise<void> {
    const requestGeneration = ++generation;
    modelLoad?.cancel();
    loadingModels = true;
    loadError = '';
    // The deadline bounds the state change, not only the HTTP call: the shared
    // admin client can wait on a cross-tab session refresh that never observes
    // this signal, so an abort alone would still leave the page spinning.
    onConnectionChange('loading');
    const race = raceWithDeadline(
      (signal) => api.chatModels(signal),
      MODEL_LIST_TIMEOUT_MS,
    );
    modelLoad = race;
    const outcome = await race.outcome;
    if (requestGeneration !== generation || outcome.kind === 'cancelled') return;
    if (modelLoad === race) modelLoad = null;
    if (outcome.kind === 'ok') {
      // A truncated or malformed payload must not reach the picker as a
      // non-array the user cannot act on.
      modelOptions = Array.isArray(outcome.value?.models) ? outcome.value.models : [];
      modelsTruncated = outcome.value?.truncated === true;
      loadError = '';
      // The composer never starts on an empty target: the loaded list decides
      // the model, preferring the last one this operator used.
      draft = { ...draft, modelId: defaultModelId(modelOptions, draft.modelId, loadLastModelId()) };
      onConnectionChange('loaded');
    } else {
      loadError =
        outcome.kind === 'timeout'
          ? tr('The model list took too long to load.')
          : localizedError(outcome.error, 'Could not load the model list.', tr);
      onConnectionChange('error');
    }
    loadingModels = false;
  }

  function resolveProviderId(modelId: string): string {
    return modelOptions.find((option) => option.id === modelId)?.provider_id ?? '';
  }

  /** Keeps the page draft and the remembered choice in step with the picker. */
  function selectModel(modelId: string): void {
    draft = { ...draft, modelId };
    saveLastModelId(modelId);
  }

  /** Batches streaming deltas into one conversation update per frame. */
  function makeStreamBuffer(): ReturnType<typeof createDeltaBuffer> {
    return createDeltaBuffer((text, reasoning) => {
      conversation = conversation.map((message) =>
        message.id === streamingId
          ? {
              ...message,
              text: message.text + text,
              reasoning:
                reasoning || message.reasoning
                  ? (message.reasoning ?? '') + reasoning
                  : message.reasoning,
            }
          : message,
      );
    });
  }

  /** Runs one streamed turn against the validated input and appends the reply. */
  async function runStreamTurn(input: WorkspaceChatInput, controller: AbortController): Promise<void> {
    const assistant = streamingAssistantMessage();
    streamingId = assistant.id;
    conversation = [...conversation, assistant];
    const buffer = makeStreamBuffer();
    try {
      await api.chatStream(
        input,
        {
          onTextDelta: (delta) => buffer.push(delta, ''),
          onReasoningDelta: (delta) => buffer.push('', delta),
          onDone: (done) => {
            buffer.flush();
            conversation = conversation.map((message) =>
              message.id === assistant.id
                ? {
                    ...message,
                    model: done.model || input.model,
                    finish_reason: done.finish_reason,
                    usage:
                      done.input_tokens !== null || done.output_tokens !== null
                        ? {
                            input_tokens: done.input_tokens ?? 0,
                            output_tokens: done.output_tokens ?? 0,
                          }
                        : null,
                    duration_ms: done.duration_ms,
                  }
                : message,
            );
          },
          onError: (message) => {
            buffer.flush();
            if (controller.signal.aborted) {
              // A stop is normal control flow, not a failed turn.
              sendNotice = tr('Generation stopped.');
              return;
            }
            conversation = conversation.map((turn) =>
              turn.id === assistant.id ? { ...buildErrorMessage(message), id: assistant.id } : turn,
            );
          },
        },
        controller.signal,
      );
    } catch (error) {
      if (controller.signal.aborted) {
        sendNotice = tr('Generation stopped.');
      } else {
        const detail = localizedError(error, 'The chat request failed.', tr);
        conversation = conversation.map((turn) =>
          turn.id === assistant.id ? { ...buildErrorMessage(detail), id: assistant.id } : turn,
        );
        sendError = detail;
      }
    } finally {
      buffer.flush();
      if (streamingId === assistant.id) streamingId = null;
    }
  }

  function interceptSlashCommand(text: string): boolean {
    const trimmed = text.trim();
    if (!trimmed.startsWith('/')) return false;
    const [head, ...tailParts] = trimmed.split(/\s+/);
    const tail = trimmed.slice(head.length).trim();
    switch (head) {
      case '/clear':
        handleClear();
        return true;
      case '/continue':
        handleContinue();
        return true;
      case '/compare':
        openCompare();
        return true;
      case '/library':
        libraryOpen = true;
        return true;
      case '/export-md':
        exportMarkdown();
        return true;
      case '/export-json':
        exportJson();
        return true;
      case '/print':
        printConversation();
        return true;
      case '/palette':
        paletteOpen = true;
        return true;
      case '/save': {
        const parsed = parseSlashSave(trimmed);
        if (!parsed) {
          sendNotice = tr('Prompt name');
          return true;
        }
        const prompt = makePrompt(parsed.name, parsed.body);
        savePrompts([prompt, ...loadPrompts()].slice(0, 200));
        sendNotice = tr('Saved.');
        return true;
      }
      case '/use': {
        const prompt = parseSlashUse(trimmed, loadPrompts());
        if (prompt) {
          draft = { ...draft, systemPrompt: prompt.body };
          sendNotice = `${tr('Apply')}: ${prompt.name}`;
        } else {
          sendNotice = tr('No saved prompts yet.');
        }
        return true;
      }
      default:
        // Unknown slash commands stay visible text so the operator can tell
        // a typo from a command.
        return false;
    }
  }

  async function handleSend(nextDraft: ChatDraft): Promise<void> {
    if (sending) return;
    sendError = '';
    sendNotice = '';
    if (interceptSlashCommand(nextDraft.message)) {
      draft = { ...nextDraft, message: '' };
      return;
    }
    // Editing a past turn branches the thread: the turn replaces itself and
    // everything after it is dropped before the resend.
    let working = conversation;
    if (editingIndex !== null) {
      working = branchFromEdit(conversation, editingIndex, nextDraft.message.trim());
      editingIndex = null;
    }
    // The prior turns are context, so validate against the (possibly branched)
    // thread before the new message is appended to it.
    const validated = validateDraftWith(
      { ...nextDraft, providerId: resolveProviderId(nextDraft.modelId) },
      working,
    );
    if (!validated.ok) {
      sendError = tr(validated.errorKey);
      return;
    }
    const controller = new AbortController();
    chatController = controller;
    sending = true;
    const userMessage = userMessageFromDraft(nextDraft);
    conversation = [...working, userMessage];
    // Clear only the consumed turn bubbles; settings and attachments stay.
    draft = { ...nextDraft, message: '', attachments: [] };
    await runStreamTurn(validated.input, controller);
    if (chatController === controller) chatController = null;
    sending = false;
  }

  function validateDraftWith(d: ChatDraft, context: ChatConversationMessage[]) {
    return validateDraft(d, context);
  }

  /** Cancels the in-flight turn; the relay drops the upstream request. */
  function stopGeneration(): void {
    chatController?.abort();
  }

  function applySuggestion(body: string): void {
    if (editingIndex !== null) {
      cancelEdit();
    }
    draft = { ...draft, message: body };
  }

  function handleClear(): void {
    stopGeneration();
    conversation = [];
    sendError = '';
    sendNotice = '';
    streamingId = null;
    editingIndex = null;
  }

  /* ---------------- Regenerate / continue ---------------- */

  async function handleRegenerate(): Promise<void> {
    if (sending) return;
    const base = dropTrailingAssistantTurns(conversation);
    if (!base.length || !draft.modelId) return;
    const lastUser = [...base].reverse().find((message) => message.role === 'user');
    if (!lastUser) return;
    const validated = validateDraft({ ...draft, providerId: resolveProviderId(draft.modelId), message: lastUser.text, attachments: [] }, contextBeforeFinalUserTurn([...base, lastUser]));
    if (!validated.ok) {
      sendError = tr(validated.errorKey);
      return;
    }
    const controller = new AbortController();
    chatController = controller;
    sending = true;
    conversation = base;
    await runStreamTurn(validated.input, controller);
    if (chatController === controller) chatController = null;
    sending = false;
  }

  async function handleContinue(): Promise<void> {
    if (sending || !draft.modelId) return;
    const validated = validateDraft(
      { ...draft, providerId: resolveProviderId(draft.modelId), message: continueInstruction(), attachments: [] },
      conversation,
    );
    if (!validated.ok) {
      sendError = tr(validated.errorKey);
      return;
    }
    const controller = new AbortController();
    chatController = controller;
    sending = true;
    await runStreamTurn(validated.input, controller);
    if (chatController === controller) chatController = null;
    sending = false;
  }

  /* ---------------- Edit and branch ---------------- */

  function startEdit(index: number): void {
    const message = conversation[index];
    if (!message || message.role !== 'user') return;
    editingIndex = index;
    draft = { ...draft, message: message.text };
  }

  function cancelEdit(): void {
    editingIndex = null;
  }

  /* ---------------- Quote ---------------- */

  function quoteTurn(text: string): void {
    draft = { ...draft, message: `> ${markdownPreview(text, 200)}\n\n` };
  }

  /* ---------------- Compare ---------------- */

  function openCompare(): void {
    if (!draft.modelId || !draft.message.trim()) {
      sendNotice = tr('Pick two to four models to compare on this exact prompt.');
      return;
    }
    const validated = validateDraft({ ...draft, providerId: resolveProviderId(draft.modelId) }, conversation);
    if (!validated.ok) {
      sendError = tr(validated.errorKey);
      return;
    }
    compareInput = validated.input;
    compareOpen = true;
  }

  /* ---------------- Export / copy / print ---------------- */

  function exportMarkdown(): void {
    downloadTextFile(
      conversationMarkdownFilename(),
      conversationToMarkdown(conversation, tr('Workspace chat')),
      'text/markdown;charset=utf-8',
    );
  }

  function exportJson(): void {
    downloadTextFile(conversationJsonFilename(), conversationToJson(conversation), 'application/json');
  }

  async function copyConversation(): Promise<void> {
    try {
      await navigator.clipboard?.writeText(conversationToMarkdown(conversation, tr('Workspace chat')));
      sendNotice = tr('Copied');
    } catch {
      // Clipboard unavailable; the export actions still work.
    }
  }

  /* ---------------- Prompt library / palette ---------------- */

  function applyPromptBody(body: string): void {
    draft = { ...draft, systemPrompt: body };
  }

  function handlePaletteCommand(command: 'clear' | 'continue' | 'compare' | 'library' | 'export-md' | 'export-json' | 'print'): void {
    switch (command) {
      case 'clear': handleClear(); break;
      case 'continue': void handleContinue(); break;
      case 'compare': openCompare(); break;
      case 'library': libraryOpen = true; break;
      case 'export-md': exportMarkdown(); break;
      case 'export-json': exportJson(); break;
      case 'print': printConversation(); break;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if ((event.metaKey || event.ctrlKey) && (event.key === 'k' || event.key === 'K')) {
      event.preventDefault();
      paletteOpen = !paletteOpen;
    }
  }

  onMount(() => {
    void loadModels();
  });
  onDestroy(() => {
    generation += 1;
    modelLoad?.cancel();
    modelLoad = null;
    // Aborting an in-flight stream on unmount releases the upstream request.
    chatController?.abort();
    chatController = null;
  });
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="chat-workspace">
  <header class="chat-topbar">
    <div class="chat-topbar-leading">
      {#if conversation.length}
        <button class="chat-icon-action" type="button" onclick={handleClear} disabled={sending} title={tr('Clear conversation')}>
          <Trash2 size={14} />
        </button>
      {/if}
      {#if conversation.length}
        <button class="chat-icon-action" type="button" onclick={exportMarkdown} disabled={sending} title={tr('Export conversation as Markdown')}>
          <FileDown size={14} />
        </button>
        <button class="chat-icon-action" type="button" onclick={exportJson} disabled={sending} title={tr('Export conversation as JSON')}>
          <Braces size={14} />
        </button>
        <button class="chat-icon-action" type="button" onclick={printConversation} title={tr('Print conversation')}>
          <Printer size={14} />
        </button>
        <button class="chat-icon-action" type="button" onclick={copyConversation} title={tr('Copy conversation')}>
          <SquareStack size={14} />
        </button>
      {/if}
      <button class="chat-icon-action" type="button" onclick={() => (libraryOpen = !libraryOpen)} title={tr('Prompt library')}>
        <Library size={14} />
      </button>
      <button class="chat-icon-action" type="button" onclick={openCompare} title={tr('Compare models')}>
        <Columns2 size={14} />
      </button>
      <button class="chat-icon-action" type="button" onclick={() => (paletteOpen = true)} title={tr('Type / for commands')}>
        <Command size={14} />
      </button>
    </div>
    <div class="chat-topbar-trailing">
      {#if modelsTruncated}
        <span class="chat-topbar-note" role="status">{tr('Model list truncated')}</span>
      {/if}
      {#if conversation.length}
        <ChatSessionMeter
          {tr}
          {locale}
          {tokens}
          {cost}
          {historySeverity}
          historyTurns={historySlice.turns}
          maxHistoryTurns={MAX_HISTORY_TURNS}
        />
      {/if}
    </div>
  </header>

  {#if editingIndex !== null}
    <div class="chat-editing-bar" role="status">
      {tr('Edit and resend')} —
      <button class="chat-editing-cancel" type="button" onclick={cancelEdit}>{tr('Cancel')}</button>
    </div>
  {/if}
  <div class="chat-workspace-body">
    <ChatConversation
      {tr}
      {locale}
      messages={conversation}
      {streamingId}
      onQuote={quoteTurn}
      onEdit={startEdit}
      onRegenerate={handleRegenerate}
      onContinue={handleContinue}
      onCopied={() => (sendNotice = tr('Copied'))}
    >
      <div slot="empty">
        <ChatHero {tr} {suggestions} onApplySuggestion={applySuggestion} />
      </div>
    </ChatConversation>
  </div>
  <div class="chat-workspace-dock">
    {#if compareOpen && compareInput}
      <ChatComparePanel
        {tr}
        options={modelOptions}
        input={compareInput}
        onClose={() => (compareOpen = false)}
      />
    {/if}
    {#if libraryOpen}
      <ChatPromptLibrary
        {tr}
        systemPrompt={draft.systemPrompt}
        onApply={applyPromptBody}
        onClose={() => (libraryOpen = false)}
      />
    {/if}
    <ChatComposer
      {tr}
      disabled={sending}
      {sending}
      {draft}
      onDraftChange={(next) => (draft = next)}
      onSend={handleSend}
      onStop={stopGeneration}
    >
      <!-- The model sits with the other send controls, not in the top bar, so
           the choice is next to the message it applies to. -->
      <div slot="model-picker" class="chat-composer-model-control">
        {#if loadingModels}
          <span class="chat-model-note" role="status">{tr('Loading saved models…')}</span>
        {:else if !loadError}
          <ChatModelPicker
            {tr}
            options={modelOptions}
            disabled={sending}
            modelId={draft.modelId}
            onModelIdChange={selectModel}
          />
        {/if}
      </div>
    </ChatComposer>
    {#if loadError}
      <ChatNotice tone="model" message={loadError} {tr} onRetry={loadModels} />
    {/if}
    {#if sendError}<ChatNotice tone="error" message={sendError} {tr} />{/if}
    {#if sendNotice}<ChatNotice tone="notice" message={sendNotice} {tr} />{/if}
  </div>
</div>

{#if paletteOpen}
  <ChatCommandPalette
    {tr}
    onCommand={handlePaletteCommand}
    onApplyPrompt={(prompt) => applyPromptBody(prompt.body)}
    onClose={() => (paletteOpen = false)}
  />
{/if}

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
    gap: 4px;
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

  .chat-editing-bar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 16px;
    border-bottom: 2px dashed var(--ink);
    background: #fff7ed;
    font-size: 11px;
    color: #b45309;
    font-weight: var(--font-semibold);
  }

  .chat-editing-cancel {
    padding: 2px 8px;
    border: 1.5px solid var(--ink);
    border-radius: 8px;
    background: var(--paper);
    color: var(--ink);
    font-size: 10px;
    cursor: pointer;
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

  .chat-composer-model-control {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    width: 100%;
    min-width: 0;
  }

  .chat-model-note { font-size: 11px; font-weight: var(--font-semibold); color: var(--muted); white-space: nowrap; }

  .chat-workspace-dock > :global(*) {
    max-width: 760px;
    margin-left: auto;
    margin-right: auto;
  }

  :global(:root[data-theme='dark']) .chat-workspace { background: rgba(14, 16, 26, 0.72); }
  :global(:root[data-theme='dark']) .chat-topbar { background: rgba(249, 115, 22, 0.08); }
  :global(:root[data-theme='dark']) .chat-icon-action { background: rgba(255, 255, 255, 0.04); }
  :global(:root[data-theme='dark']) .chat-editing-bar { background: rgba(249, 115, 22, 0.1); }

  /* Print: only the conversation, sized to the paper. */
  @media print {
    :global(body *) {
      visibility: hidden !important;
    }
    .chat-workspace,
    .chat-workspace * {
      visibility: visible !important;
    }
    .chat-workspace {
      position: absolute;
      inset: 0;
      height: auto;
      border: none;
      box-shadow: none;
      overflow: visible;
      background: #fff;
    }
    .chat-topbar,
    .chat-editing-bar,
    .chat-workspace-dock {
      display: none !important;
    }
    .chat-workspace-body {
      overflow: visible;
      height: auto;
    }
  }

</style>
