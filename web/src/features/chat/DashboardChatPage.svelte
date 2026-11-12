<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { streamWorkspaceChat } from '../../lib/api/workspace-chat-stream';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import SelectSearch from '../../components/SelectSearch.svelte';
  import Input from '../../components/Input.svelte';
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

<div class="page-surface flex flex-col min-h-0">
  <div class="surface flex min-h-[480px] h-[calc(100svh-320px)] overflow-hidden max-nav:h-auto max-nav:min-h-0 max-nav:flex-col">
    <aside class="flex w-[250px] shrink-0 flex-col gap-2.5 border-r border-line bg-surface-sunken p-3 max-nav:order-2 max-nav:w-full max-nav:border-r-0 max-nav:border-t max-nav:border-line">
      <Button class="flex items-center justify-center gap-[7px] min-h-[38px] rounded-xl border border-line bg-surface text-ink text-dash-body font-medium cursor-pointer transition-colors duration-150 hover:border-accent hover:text-accent hover:bg-db-accent-tint focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring" type="button" onclick={newChat}>
        <svg class="icon"><use href="#i-plus" /></svg>
        {tr('New conversation')}
      </Button>
      <div class="grid flex-1 gap-1.5 overflow-y-auto content-start">
        {#each sessions as session (session.id)}
          <button
            type="button"
            onclick={() => selectSession(session.id)}
            class="grid gap-[3px] rounded-[11px] border bg-transparent py-2.5 px-3 text-left cursor-pointer transition-colors duration-150 focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring {session.id === activeSessionId ? 'bg-surface border-line shadow-sm' : 'border-transparent hover:bg-surface'}"
          >
            <strong class="overflow-hidden text-ellipsis whitespace-nowrap text-ink text-label font-semibold">{session.title}</strong>
            <span class="text-muted text-caption">{tr('{label} · {count} messages', { label: session.modelLabel || tr('no model'), count: session.messages.length })}</span>
          </button>
        {:else}
          <div class="grid flex-1 place-items-center p-6 text-center">
            <p class="m-0 text-muted text-meta">{tr('No conversations yet.')}</p>
          </div>
        {/each}
      </div>
    </aside>

    <section class="flex min-w-0 min-h-0 flex-1 flex-col bg-db-tint-strong max-nav:min-h-[60svh]">
      <div class="flex items-center gap-2.5 border-b border-line bg-surface px-4 py-2">
        <span class="shrink-0 text-muted text-caption font-semibold uppercase tracking-[0.04em]">{tr('Model')}</span>
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

      <div class="flex-1 min-h-0 overflow-y-auto p-[18px] flex flex-col gap-3 {messages.length ? '[scrollbar-width:thin] [scrollbar-color:var(--text-dim)_transparent]' : '[scrollbar-width:none]'}" bind:this={messagesEl} aria-live="polite">
        {#if messages.length === 0}
          <div class="m-auto grid justify-items-center gap-3 px-5 py-[30px] text-center">
            <span class="grid size-12 place-items-center rounded-2xl border border-line bg-surface text-muted"><svg class="icon size-6!"><use href="#i-chat" /></svg></span>
            <div class="grid gap-1">
              <strong class="text-ink text-section-title">{tr('Start a conversation')}</strong>
              <p class="m-0 text-dash-body text-muted">{tr('Messages are routed through the gateway exactly like production traffic.')}</p>
            </div>
            <div class="flex flex-wrap justify-center gap-2">
              {#each [
                { label: tr('Summarize this text in 3 bullets'), text: 'Summarize this text in 3 bullets: ExoRoute is a protocol gateway that normalizes client and provider formats, adds retries, combos, quota tracking, and per-key scoping.' },
                { label: tr('Write a Python retry decorator'), text: 'Write a Python retry decorator with exponential backoff' },
                { label: tr('Explain SSE vs WebSocket'), text: 'Explain SSE vs WebSocket for LLM streaming' },
              ] as suggestion (suggestion.label)}
                <button
                  type="button"
                  class="rounded-full border border-line bg-surface px-3.5 py-2 text-muted text-meta cursor-pointer transition-[border-color,color,background-color] duration-150 hover:border-accent hover:text-accent hover:bg-db-accent-tint focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring"
                  onclick={() => void send(suggestion.text)}
                >{suggestion.label}</button>
              {/each}
            </div>
          </div>
        {/if}
        {#each messages as message (message.id)}
          <div class="flex {message.role === 'user' ? 'justify-end' : 'justify-start'}">
            <div class="max-w-[min(78%,640px)] rounded-[15px] px-[15px] py-[11px] text-dash-body leading-[1.45] whitespace-pre-wrap [word-break:break-word] {message.role === 'user' ? 'bg-ink text-canvas rounded-br-[5px]' : 'bg-surface text-ink border border-line rounded-bl-[5px]'}">{message.text}{#if message.streaming}<span class="inline-block w-2 h-3.5 ms-0.5 rounded-[2px] bg-accent align-[-2px] animate-chat-blink"></span>{/if}</div>
          </div>
        {/each}
      </div>

      <form class="mx-3.5 mb-3.5 mt-2 flex items-center gap-1.5 rounded-2xl border border-line bg-surface px-2 py-1.5 transition-[border-color,box-shadow] duration-150 focus-within:border-accent focus-within:ring-3 focus-within:ring-db-accent-ring" onsubmit={(e) => { e.preventDefault(); void send(); }}>
        <Button class="grid size-8! shrink-0 place-items-center rounded-full! border-0! bg-transparent! text-muted! cursor-pointer transition-colors duration-150 hover:bg-db-tint-strong! hover:text-ink! disabled:opacity-55 focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring [&_.icon]:size-[15px]!" type="button" title={tr('Attach image')} onclick={() => fileInput?.click()}>
          <svg class="icon"><use href="#i-paperclip" /></svg>
        </Button>
        <Input bind:element={fileInput} type="file" accept="image/*" hidden onchange={onAttach} />
        <textarea class="flex-1 min-h-8 max-h-[140px] resize-none border-0 bg-transparent! px-1.5 py-1.5 text-ink text-dash-body leading-[1.45] outline-none placeholder:text-muted"
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
          <Button class="grid size-9! shrink-0 place-items-center rounded-full! border-0! bg-db-danger! text-canvas! cursor-pointer transition-colors duration-150 hover:bg-accent-deep! disabled:opacity-55 focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring [&_.icon]:size-[14px]!" type="button" title={tr('Stop generating')} onclick={stop}>
            <svg class="icon"><use href="#i-stop" /></svg>
          </Button>
        {:else}
          <Button class="grid size-9! shrink-0 place-items-center rounded-full! border-0! bg-ink! text-canvas! cursor-pointer transition-colors duration-150 hover:bg-accent! disabled:opacity-40 focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring [&_.icon]:size-[16px]!" type="submit" title={tr('Send')} disabled={!composerText.trim() || !selectedModel}>
            <svg class="icon"><use href="#i-send" /></svg>
          </Button>
        {/if}
      </form>
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
