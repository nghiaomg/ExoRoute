<script lang="ts">
  import Button from "../../components/ui/Button.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { streamWorkspaceChat } from '../../lib/api/workspace-chat-stream';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import SelectSearch from '../../components/ui/SelectSearch.svelte';
  import Input from '../../components/ui/Input.svelte';
  import type {
    ChatAttachmentInput,
    WorkspaceChatInput,
    WorkspaceChatModelOption,
  } from '../../lib/types';

  interface Props {
    tr: Translate;
    locale: Locale;
    onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  }

  let { tr, locale: _locale, onConnectionChange }: Props = $props();

  interface ChatMessage {
    id: number;
    role: 'user' | 'assistant';
    text: string;
    streaming?: boolean;
    error?: boolean;
  }

  interface Session {
    id: number;
    title: string;
    modelLabel: string;
    messages: ChatMessage[];
  }

  let sessions = $state<Session[]>([]);
  let activeSessionId = $state<number | null>(null);
  let conversationSearch = $state('');
  let modelOptions = $state<WorkspaceChatModelOption[]>([]);
  let selectedModel = $state('');
  let messagesEl: HTMLElement;
  let inputEl: HTMLTextAreaElement;
  let composerText = $state('');
  let streaming = $state(false);
  let streamController: AbortController | null = null;
  let msgSeq = 0;
  let sessionSeq = 0;

  // Parameters dialog. The backend validates temperature in [0, 2] and
  // max_tokens in [1, 131072] (128 * 1024), and caps the system prompt at
  // 32k chars; the UI clamps to the same bounds before anything is sent.
  let paramsDialog = $state<HTMLDialogElement>();
  const CHAT_PARAMETERS_KEY = 'exoroute:chat:parameters';
  const CHAT_MAX_TOKENS_LIMIT = 131072;
  const CHAT_SYSTEM_PROMPT_LIMIT = 32 * 1024;
  const CHAT_PARAMETER_DEFAULTS = {
    temperature: 0.7,
    maxTokens: 4096,
    systemPrompt: 'You are a concise assistant testing the ExoRoute gateway.',
    streamEnabled: true,
  };
  let temperature = $state(CHAT_PARAMETER_DEFAULTS.temperature);
  let maxTokens = $state(CHAT_PARAMETER_DEFAULTS.maxTokens);
  let systemPrompt = $state(CHAT_PARAMETER_DEFAULTS.systemPrompt);
  let streamEnabled = $state(CHAT_PARAMETER_DEFAULTS.streamEnabled);

  function clampChatParameters(input: { temperature: number; maxTokens: number; systemPrompt: string; streamEnabled: boolean }) {
    return {
      temperature: Math.min(2, Math.max(0, input.temperature)),
      maxTokens: Math.min(CHAT_MAX_TOKENS_LIMIT, Math.max(1, Math.round(input.maxTokens))),
      systemPrompt: input.systemPrompt.slice(0, CHAT_SYSTEM_PROMPT_LIMIT),
      streamEnabled: Boolean(input.streamEnabled),
    };
  }

  // The testbench parameters survive reloads as a plain, non-sensitive UI
  // preference; a corrupted or half-typed record falls back field-by-field.
  function loadChatParameters(): void {
    try {
      const raw = window.localStorage.getItem(CHAT_PARAMETERS_KEY);
      if (!raw) return;
      const parsed: unknown = JSON.parse(raw);
      if (!parsed || typeof parsed !== 'object') return;
      const saved = parsed as { temperature?: unknown; maxTokens?: unknown; systemPrompt?: unknown; streamEnabled?: unknown };
      const values = clampChatParameters({
        temperature: typeof saved.temperature === 'number' ? saved.temperature : CHAT_PARAMETER_DEFAULTS.temperature,
        maxTokens: typeof saved.maxTokens === 'number' ? saved.maxTokens : CHAT_PARAMETER_DEFAULTS.maxTokens,
        systemPrompt: typeof saved.systemPrompt === 'string' ? saved.systemPrompt : CHAT_PARAMETER_DEFAULTS.systemPrompt,
        streamEnabled: typeof saved.streamEnabled === 'boolean' ? saved.streamEnabled : CHAT_PARAMETER_DEFAULTS.streamEnabled,
      });
      temperature = values.temperature;
      maxTokens = values.maxTokens;
      systemPrompt = values.systemPrompt;
      streamEnabled = values.streamEnabled;
    } catch {
      // Corrupted preference: keep the defaults instead of failing the page.
    }
  }

  function saveChatParameters(): void {
    try {
      const values = clampChatParameters({ temperature, maxTokens, systemPrompt, streamEnabled });
      temperature = values.temperature;
      maxTokens = values.maxTokens;
      systemPrompt = values.systemPrompt;
      streamEnabled = values.streamEnabled;
      window.localStorage.setItem(CHAT_PARAMETERS_KEY, JSON.stringify(values));
    } catch {
      // Storage unavailable: keep the in-memory values for this session.
    }
  }

  function resetChatParameters(): void {
    temperature = CHAT_PARAMETER_DEFAULTS.temperature;
    maxTokens = CHAT_PARAMETER_DEFAULTS.maxTokens;
    systemPrompt = CHAT_PARAMETER_DEFAULTS.systemPrompt;
    streamEnabled = CHAT_PARAMETER_DEFAULTS.streamEnabled;
    saveChatParameters();
    showDashboardToast(tr('Defaults restored'));
  }
  let pendingAttachments = $state<ChatAttachmentInput[]>([]);
  // Initialize with null, never undefined: `Input.element` declares a fallback
  // (`$bindable(null)`), and Svelte 5 throws props_invalid_value when a bind
  // passes `undefined` to a prop that has one — which crashed the whole chat
  // page at mount, leaving the shell skeleton stuck.
  let fileInput = $state<HTMLInputElement | null>(null);

  const activeSession = $derived(sessions.find((s) => s.id === activeSessionId) ?? null);
  const messages = $derived(activeSession?.messages ?? []);
  const filteredSessions = $derived(
    conversationSearch.trim()
      ? sessions.filter((session) =>
          `${session.title} ${session.modelLabel}`.toLowerCase().includes(conversationSearch.trim().toLowerCase()),
        )
      : sessions,
  );

  function selectedOption(): WorkspaceChatModelOption | undefined {
    return modelOptions.find((m) => m.id === selectedModel);
  }

  function modelLabel(option: WorkspaceChatModelOption): string {
    return `${option.model} · ${option.provider_id}`;
  }

  /** Searchable picker rows: same text as the old select, plus the full id kept searchable. */
  const modelItems = $derived(
    modelOptions.map((option) => ({
      label: option.model,
      hint: option.provider_id,
      keywords: option.id,
      value: option.id,
    })),
  );

  function scrollBottom() {
    requestAnimationFrame(() => {
      if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
    });
  }

  function patchActive(fn: (session: Session) => void) {
    sessions = sessions.map((s) => (s.id === activeSessionId ? (() => { const copy = { ...s, messages: [...s.messages] }; fn(copy); return copy; })() : s));
  }

  function patchMessage(sessionId: number, messageId: number, patch: Partial<ChatMessage>) {
    sessions = sessions.map((s) => s.id !== sessionId ? s : {
      ...s,
      messages: s.messages.map((m) => (m.id === messageId ? { ...m, ...patch } : m)),
    });
  }

  function ensureSession(): Session {
    if (activeSession) return activeSession;
    sessionSeq += 1;
    const session: Session = {
      id: sessionSeq,
      title: 'New conversation',
      modelLabel: selectedOption() ? modelLabel(selectedOption()!) : '',
      messages: [],
    };
    sessions = [...sessions, session];
    activeSessionId = session.id;
    return session;
  }

  async function loadModels() {
    try {
      const result = await api.chatModels();
      modelOptions = result.models;
      if (!selectedModel && modelOptions.length) selectedModel = modelOptions[0].id;
      onConnectionChange('loaded');
    } catch (error) {
      onConnectionChange('error');
      showDashboardToast(error instanceof Error ? error.message : tr('Could not load chat models'));
    }
  }

  function buildInput(text: string, history: { role: 'user' | 'assistant'; text: string }[]): WorkspaceChatInput {
    const option = selectedOption();
    return {
      provider_id: option?.provider_id ?? '',
      model: option?.model ?? '',
      message: text,
      system_prompt: systemPrompt.trim().slice(0, CHAT_SYSTEM_PROMPT_LIMIT) || null,
      history: history.length ? history : null,
      temperature: Math.min(2, Math.max(0, temperature)),
      max_tokens: Math.min(CHAT_MAX_TOKENS_LIMIT, Math.max(1, Math.round(maxTokens))),
      ...(pendingAttachments.length ? { attachments: pendingAttachments } : {}),
    };
  }

  async function send(rawText?: string) {
    const text = (rawText ?? composerText).trim();
    const option = selectedOption();
    if (!text || streaming || !option) return;
    if (!option.provider_id || !option.model) {
      showDashboardToast(tr('Select a model first'));
      return;
    }

    const session = ensureSession();
    if (session.title === 'New conversation') {
      session.title = text.length > 42 ? `${text.slice(0, 42)}…` : text;
      sessions = [...sessions];
    }
    session.modelLabel = modelLabel(option);

    msgSeq += 1;
    const userMsg: ChatMessage = { id: msgSeq, role: 'user', text };
    patchActive((s) => { s.messages.push(userMsg); });
    composerText = '';
    if (inputEl) inputEl.style.height = 'auto';
    scrollBottom();

    const history = session.messages
      .filter((m) => !m.streaming && !m.error && m.id !== userMsg.id)
      .slice(-12)
      .map((m) => ({ role: m.role, text: m.text }));

    const attachments = pendingAttachments;
    pendingAttachments = [];

    msgSeq += 1;
    const assistantId = msgSeq;
    patchActive((s) => { s.messages.push({ id: assistantId, role: 'assistant', text: '', streaming: true }); });
    scrollBottom();

    const input = buildInput(text, history);
    if (attachments.length) input.attachments = attachments;

    streaming = true;
    streamController = new AbortController();
    let accumulated = '';
    const startedAt = Date.now();

    const finish = (errorText?: string) => {
      streaming = false;
      streamController = null;
      const duration = ((Date.now() - startedAt) / 1000).toFixed(1);
      patchMessage(session.id, assistantId, errorText
        ? { streaming: false, error: true, text: accumulated || errorText }
        : { streaming: false, text: accumulated });
      if (!errorText) showDashboardToast(tr('Responded via {model} ({duration}s)', { model: option.model, duration }));
      scrollBottom();
    };

    if (!streamEnabled) {
      try {
        const result = await api.chat(input, streamController.signal);
        accumulated = result.reply;
        finish();
      } catch (error) {
        finish(error instanceof Error ? error.message : tr('Chat request failed'));
      }
      return;
    }

    try {
      await streamWorkspaceChat(input, {
        onTextDelta: (delta) => {
          accumulated += delta;
          patchMessage(session.id, assistantId, { text: accumulated });
          scrollBottom();
        },
        onDone: () => finish(),
        onError: (message) => finish(message),
      }, streamController.signal);
    } catch (error) {
      if (!streamController?.signal.aborted) finish(error instanceof Error ? error.message : tr('Chat stream failed'));
    }
  }

  function stop() {
    streamController?.abort();
    streamController = null;
    streaming = false;
    if (activeSession) {
      patchMessage(activeSession.id, activeSession.messages[activeSession.messages.length - 1]?.id ?? -1, { streaming: false });
      const last = activeSession.messages[activeSession.messages.length - 1];
      if (last?.streaming) {
        sessions = sessions.map((s) => s.id !== activeSession.id ? s : {
          ...s,
          messages: s.messages.map((m) => m.id === last.id ? { ...m, streaming: false, text: tr('{text} ▌[stopped]', { text: m.text }) } : m),
        });
      }
    }
    showDashboardToast(tr('Generation stopped'));
  }

  function newChat() {
    stop();
    activeSessionId = null;
    showDashboardToast(tr('New conversation started'));
  }

  function selectSession(id: number) {
    activeSessionId = id;
  }

  function autoGrow() {
    if (!inputEl) return;
    inputEl.style.height = 'auto';
    inputEl.style.height = `${Math.min(inputEl.scrollHeight, 140)}px`;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  }

  async function onAttach(event: Event) {
    const file = (event.currentTarget as HTMLInputElement).files?.[0];
    (event.currentTarget as HTMLInputElement).value = '';
    if (!file) return;
    if (!file.type.startsWith('image/')) {
      showDashboardToast(tr('Only image attachments are supported here'));
      return;
    }
    if (file.size > 4 * 1024 * 1024) {
      showDashboardToast(tr('Attachment exceeds 4 MB'));
      return;
    }
    const dataUrl = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
    pendingAttachments = [...pendingAttachments, {
      kind: 'image',
      media_type: file.type,
      data: dataUrl.split(',')[1] ?? '',
      filename: file.name,
    }];
    showDashboardToast(tr('Attached {name}', { name: file.name }));
  }

  onMount(() => { loadChatParameters(); void loadModels(); });
  onDestroy(() => { streamController?.abort(); });
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>{tr('Chat')}</h1>
    <p>{tr('Test models through the gateway before wiring them into production.')}</p>
  </div>
  <div class="heading-actions">
    <Button class="action-button" type="button" data-open-modal="chat-settings-dialog">
      <span class="button-icon"><svg class="icon"><use href="#i-sliders" /></svg></span>
      <span class="button-label">{tr('Parameters')}</span>
    </Button>
  </div>
</div>

<div class="page-surface chat-page flex flex-col min-h-0">
  <div class="chat-shell">
    <aside class="chat-history" aria-label={tr('Conversations')}>
      <button class="chat-new-button" type="button" onclick={newChat}>
        <svg class="icon"><use href="#i-plus" /></svg>
        <span>{tr('Cuộc trò chuyện mới')}</span>
      </button>
      <label class="chat-history-search">
        <svg class="icon" aria-hidden="true"><use href="#i-search" /></svg>
        <input type="search" bind:value={conversationSearch} placeholder={tr('Search conversations')} aria-label={tr('Search conversations')} />
      </label>
      <div class="chat-history-list">
        {#each filteredSessions as session (session.id)}
          <button
            type="button"
            onclick={() => selectSession(session.id)}
            class="chat-history-item"
            class:active={session.id === activeSessionId}
            aria-current={session.id === activeSessionId ? 'true' : undefined}
          >
            <strong>{session.title}</strong>
            <span>{tr('{label} · {count} messages', { label: session.modelLabel || tr('no model'), count: session.messages.length })}</span>
          </button>
        {:else}
          <div class="chat-history-empty">
            <svg class="icon" aria-hidden="true"><use href="#i-chat" /></svg>
            <p>{tr('No conversations yet.')}</p>
            <small>{tr('Conversations you start appear here.')}</small>
          </div>
        {/each}
      </div>
    </aside>

    <section class="chat-workspace">
      <div class="chat-toolbar">
        <span class="chat-toolbar-label">{tr('Model')}</span>
        <div class="min-w-0 max-w-[520px] flex-1">
          <SelectSearch
            items={modelItems}
            value={selectedModel}
            disabled={streaming}
            pill
            clearable={false}
            openOnClick
            ariaLabel={tr('Select a model')}
            placeholder={tr('Select a model')}
            noOptionsText={tr('No models found yet')}
            onValueChange={(next) => (selectedModel = next)}
          />
        </div>
        {#if pendingAttachments.length}
          <span class="ms-auto text-muted text-caption">{tr('{count} attachments queued', { count: pendingAttachments.length })}</span>
        {/if}
      </div>

      <div class="chat-messages" bind:this={messagesEl} aria-live="polite">
        {#if messages.length === 0}
          <div class="chat-empty">
            <span class="chat-empty-icon"><svg class="icon"><use href="#i-chat" /></svg></span>
            <h2>{tr('Start chatting with AI')}</h2>
            <p>{tr('Experiment with AI models through the ExoRoute Gateway.')}</p>
            <div class="chat-suggestions">
              {#each [
                { label: tr('Summarize text into 3 bullet points'), text: 'Summarize this text in 3 bullet points: ExoRoute is a protocol gateway that normalizes client and provider formats, adds retries, combos, quota tracking, and per-key scoping.' },
                { label: tr('Write a Python retry decorator'), text: 'Write a Python retry decorator with exponential backoff' },
                { label: tr('Explain SSE vs WebSocket'), text: 'Explain SSE vs WebSocket for LLM streaming' },
              ] as suggestion (suggestion.label)}
                <button type="button" class="chat-suggestion" onclick={() => void send(suggestion.text)}>{suggestion.label}</button>
              {/each}
            </div>
          </div>
        {/if}
        {#each messages as message (message.id)}
          <div class="chat-message-row" class:user={message.role === 'user'}>
            <div class="chat-bubble" class:error={message.error}>{message.text}{#if message.streaming}<span class="chat-caret" aria-hidden="true"></span>{/if}</div>
          </div>
        {/each}
      </div>

      <div class="chat-composer-dock">
        <form class="chat-composer" onsubmit={(e) => { e.preventDefault(); void send(); }}>
          <Button class="chat-attach" type="button" title={tr('Attach image')} onclick={() => fileInput?.click()}>
            <svg class="icon"><use href="#i-paperclip" /></svg>
          </Button>
          <Input bind:element={fileInput} type="file" accept="image/*" hidden onchange={onAttach} />
          <textarea class="chat-textarea"
            bind:this={inputEl}
            bind:value={composerText}
            rows="1"
            placeholder={tr('Message the model… (Enter to send, Shift+Enter for newline)')}
            aria-label={tr('Message input')}
            onkeydown={onKeydown}
            oninput={autoGrow}
            disabled={streaming}
          ></textarea>
          {#if streaming}
            <Button class="chat-send stop" type="button" title={tr('Stop generating')} onclick={stop}>
              <svg class="icon"><use href="#i-stop" /></svg>
            </Button>
          {:else}
            <Button class="chat-send" type="submit" title={tr('Send')} disabled={!composerText.trim() || !selectedModel}>
              <svg class="icon"><use href="#i-send" /></svg>
            </Button>
          {/if}
        </form>
      </div>
    </section>
  </div>
</div>

<!-- Chat parameters dialog; the shell opens it via [data-open-modal] on the Parameters button -->
<dialog class="modal" id="chat-settings-dialog" bind:this={paramsDialog} aria-labelledby="chat-settings-title">
  <form class="modal-card" onsubmit={(e) => { e.preventDefault(); saveChatParameters(); paramsDialog?.close(); showDashboardToast(tr('Parameters applied to the next request')); }}>
    <div class="modal-heading">
      <div>
        <h2 id="chat-settings-title">{tr('Chat parameters')}</h2>
        <p>{tr('Applied to the next request through the gateway.')}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="modal-fields grid-cols-2! max-nav:grid-cols-1!">
      <label class="field">
        {tr('Temperature')}
        <Input class="w-full! accent-accent!" type="range" min="0" max="2" step="0.1" bind:value={temperature} />
        <span class="text-muted text-meta tabular-nums">{temperature.toFixed(1)}</span>
      </label>
      <label class="field">
        {tr('Max tokens')}
        <Input type="number" min="64" max="128000" step="64" bind:value={maxTokens} />
      </label>
      <label class="field prov-field-wide">
        {tr('System prompt')}
        <textarea rows="3" placeholder={tr('You are a helpful routing assistant…')} bind:value={systemPrompt}></textarea>
      </label>
      <label class="field flex! flex-row! items-center gap-2 text-muted text-dash-body!">
        <!-- w/h overrides: the legacy `.modal-form input` rule would otherwise
            stretch this checkbox to a 100%-wide 37px control. -->
        <Input class="w-4! h-4! shrink-0 accent-accent!" type="checkbox" bind:checked={streamEnabled} />
        <span class="flex-1">{tr('Stream response (SSE)')}</span>
      </label>
    </div>
    <div class="modal-actions">
      <Button class="page-action" type="button" onclick={resetChatParameters}>{tr('Reset to defaults')}</Button>
      <Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button>
      <Button class="page-action primary" type="submit">{tr('Apply')}</Button>
    </div>
  </form>
</dialog>

<style>
  /* Scoped ExoRoute Chat workspace: clean SaaS surfaces, minimal borders. */
  .chat-page { min-height: 0; }
  .chat-shell {
    display: grid;
    grid-template-columns: 268px minmax(0, 1fr);
    min-height: 480px;
    height: calc(100svh - 320px);
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 14px;
    background: var(--surface);
  }

  .chat-history {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
    min-height: 0;
    padding: 14px 12px;
    border-right: 1px solid var(--line);
    background: var(--surface);
  }
  :global(.chat-new-button) {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    box-sizing: border-box;
    height: 40px;
    flex: 0 0 40px;
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    border-radius: 10px;
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
    color: var(--accent-strong);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color .15s ease, border-color .15s ease;
  }
  :global(.chat-new-button:hover) { background: color-mix(in srgb, var(--accent) 14%, var(--surface)); border-color: var(--accent); }
  .chat-history-search {
    display: flex;
    box-sizing: border-box;
    height: 36px;
    flex: 0 0 36px;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--surface-sunken);
    color: var(--muted);
  }
  .chat-history-search:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 12%, transparent); }
  .chat-history-search input { width: 100%; min-width: 0; border: 0; background: transparent; color: var(--ink); font-size: 12px; outline: none; }
  .chat-history-list { display: grid; align-content: start; gap: 4px; flex: 1; min-height: 0; overflow-y: auto; }
  .chat-history-item {
    display: grid;
    gap: 3px;
    width: 100%;
    padding: 10px 11px;
    border: 1px solid transparent;
    border-radius: 9px;
    background: transparent;
    text-align: left;
    cursor: pointer;
    transition: background-color .15s ease;
  }
  .chat-history-item:hover { background: var(--surface-sunken); }
  .chat-history-item.active { border-color: color-mix(in srgb, var(--accent) 24%, var(--line)); background: color-mix(in srgb, var(--accent) 8%, var(--surface)); }
  .chat-history-item strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink); font-size: 13px; font-weight: 600; }
  .chat-history-item.active strong { color: var(--accent-strong); }
  .chat-history-item span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted); font-size: 11px; }
  .chat-history-empty { display: grid; justify-items: center; gap: 6px; padding: 32px 12px; text-align: center; color: var(--muted); }
  .chat-history-empty p { margin: 0; font-size: 12px; font-weight: 600; color: var(--ink); }
  .chat-history-empty small { font-size: 11px; line-height: 1.45; }

  .chat-workspace { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--canvas); }
  .chat-toolbar { display: flex; align-items: center; gap: 10px; flex: 0 0 auto; min-height: 48px; padding: 8px 16px; border-bottom: 1px solid var(--line); background: var(--surface); }
  .chat-toolbar-label { flex: 0 0 auto; color: var(--muted); font-size: 11px; font-weight: 600; letter-spacing: .04em; text-transform: uppercase; }
  .chat-messages { display: flex; flex-direction: column; gap: 14px; flex: 1; min-height: 0; overflow-y: auto; padding: 20px 18px; }
  .chat-message-row { display: flex; justify-content: flex-start; }
  .chat-message-row.user { justify-content: flex-end; }
  .chat-bubble { max-width: min(78%, 680px); padding: 11px 14px; border: 1px solid var(--line); border-radius: 12px; border-bottom-left-radius: 4px; background: var(--surface); color: var(--ink); font-size: 13px; line-height: 1.5; white-space: pre-wrap; word-break: break-word; }
  .chat-message-row.user .chat-bubble { border-color: var(--accent); border-radius: 12px; border-bottom-right-radius: 4px; background: var(--accent); color: var(--surface); }
  .chat-bubble.error { border-color: color-mix(in srgb, var(--danger, #dc2626) 45%, var(--line)); }
  .chat-caret { display: inline-block; width: 6px; height: 13px; margin-left: 3px; border-radius: 2px; background: var(--accent); vertical-align: -2px; animation: chat-blink 1s step-end infinite; }
  @keyframes chat-blink { 50% { opacity: 0; } }

  .chat-empty { display: grid; justify-items: center; align-content: center; gap: 10px; flex: 1; padding: 24px 16px; text-align: center; }
  .chat-empty-icon { display: grid; place-items: center; width: 52px; height: 52px; border-radius: 15px; background: var(--surface-sunken); color: var(--accent); }
  .chat-empty-icon .icon { width: 24px; height: 24px; }
  .chat-empty h2 { margin: 4px 0 0; color: var(--ink); font-size: 19px; font-weight: 650; }
  .chat-empty p { margin: 0; color: var(--muted); font-size: 13px; }
  .chat-suggestions { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; width: min(100%, 640px); margin-top: 12px; }
  .chat-suggestion { padding: 12px 14px; border: 1px solid var(--line); border-radius: 10px; background: var(--surface); color: var(--ink); font-size: 12px; text-align: left; cursor: pointer; transition: border-color .15s ease, color .15s ease, background-color .15s ease; }
  .chat-suggestion:hover { border-color: var(--accent); color: var(--accent-strong); background: color-mix(in srgb, var(--accent) 6%, var(--surface)); }

  .chat-composer-dock { flex: 0 0 auto; padding: 10px 16px 14px; border-top: 1px solid var(--line); background: var(--surface); }
  .chat-composer { display: flex; align-items: flex-end; gap: 8px; max-width: 760px; margin: 0 auto; padding: 8px; border: 1px solid var(--line); border-radius: 12px; background: var(--surface); box-shadow: 0 1px 3px color-mix(in srgb, var(--ink) 6%, transparent); transition: border-color .15s ease, box-shadow .15s ease; }
  .chat-composer:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 12%, transparent); }
  :global(.chat-attach) { display: grid; place-items: center; flex: 0 0 auto; width: 34px; height: 34px; border: 0; border-radius: 9px; background: transparent; color: var(--muted); cursor: pointer; }
  :global(.chat-attach:hover) { background: var(--surface-sunken); color: var(--ink); }
  .chat-textarea { flex: 1; min-width: 0; min-height: 34px; max-height: 140px; padding: 7px 4px; border: 0; background: transparent; color: var(--ink); font-size: 13px; line-height: 1.5; resize: none; outline: none; }
  .chat-textarea::placeholder { color: var(--muted); }
  .chat-textarea:disabled { opacity: .6; }
  :global(.chat-send) { display: grid; place-items: center; flex: 0 0 auto; width: 36px; height: 36px; border: 0; border-radius: 10px; background: var(--accent); color: var(--surface); cursor: pointer; transition: background-color .15s ease, opacity .15s ease; }
  :global(.chat-send:hover:not(:disabled)) { background: var(--accent-strong); }
  :global(.chat-send:disabled) { opacity: .45; cursor: not-allowed; }
  :global(.chat-send.stop) { background: color-mix(in srgb, var(--danger, #dc2626) 85%, var(--ink)); }

  @media (max-width: 920px) {
    .chat-shell { grid-template-columns: 1fr; height: auto; min-height: 70svh; }
    .chat-history { border-right: 0; border-bottom: 1px solid var(--line); max-height: 180px; }
    .chat-history-list { max-height: 90px; }
    .chat-suggestions { grid-template-columns: 1fr; }
  }
  @media (max-width: 640px) {
    .chat-shell { border-radius: 10px; }
    .chat-toolbar, .chat-messages, .chat-composer-dock { padding-left: 12px; padding-right: 12px; }
    .chat-bubble { max-width: 90%; }
  }
</style>
