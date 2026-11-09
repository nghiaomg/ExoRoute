<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { streamWorkspaceChat } from '../../lib/api/workspace-chat-stream';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type {
    ChatAttachmentInput,
    WorkspaceChatInput,
    WorkspaceChatModelOption,
  } from '../../lib/types';
  import '../../styles/dashboard/pages/chat.css';

  interface Props {
    tr: Translate;
    locale: Locale;
    onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  }

  let { tr: _tr, locale: _locale, onConnectionChange }: Props = $props();

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

  // Parameters dialog
  let paramsDialog = $state<HTMLDialogElement>();
  let temperature = $state(0.7);
  let maxTokens = $state(4096);
  let systemPrompt = $state('You are a concise assistant testing the ExoRoute gateway.');
  let streamEnabled = $state(true);
  let pendingAttachments = $state<ChatAttachmentInput[]>([]);
  let fileInput: HTMLInputElement;

  const activeSession = $derived(sessions.find((s) => s.id === activeSessionId) ?? null);
  const messages = $derived(activeSession?.messages ?? []);

  function selectedOption(): WorkspaceChatModelOption | undefined {
    return modelOptions.find((m) => m.id === selectedModel);
  }

  function modelLabel(option: WorkspaceChatModelOption): string {
    return `${option.model} · ${option.provider_id}`;
  }

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
      showDashboardToast(error instanceof Error ? error.message : 'Could not load chat models');
    }
  }

  function buildInput(text: string, history: { role: 'user' | 'assistant'; text: string }[]): WorkspaceChatInput {
    const option = selectedOption();
    return {
      provider_id: option?.provider_id ?? '',
      model: option?.model ?? '',
      message: text,
      system_prompt: systemPrompt.trim() || null,
      history: history.length ? history : null,
      temperature,
      max_tokens: maxTokens,
      ...(pendingAttachments.length ? { attachments: pendingAttachments } : {}),
    };
  }

  async function send(rawText?: string) {
    const text = (rawText ?? composerText).trim();
    const option = selectedOption();
    if (!text || streaming || !option) return;
    if (!option.provider_id || !option.model) {
      showDashboardToast('Select a model first');
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
      if (!errorText) showDashboardToast(`Responded via ${option.model} (${duration}s)`);
      scrollBottom();
    };

    if (!streamEnabled) {
      try {
        const result = await api.chat(input, streamController.signal);
        accumulated = result.reply;
        finish();
      } catch (error) {
        finish(error instanceof Error ? error.message : 'Chat request failed');
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
      if (!streamController?.signal.aborted) finish(error instanceof Error ? error.message : 'Chat stream failed');
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
          messages: s.messages.map((m) => m.id === last.id ? { ...m, streaming: false, text: `${m.text} ▌[stopped]` } : m),
        });
      }
    }
    showDashboardToast('Generation stopped');
  }

  function newChat() {
    stop();
    activeSessionId = null;
    showDashboardToast('New conversation started');
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
      showDashboardToast('Only image attachments are supported here');
      return;
    }
    if (file.size > 4 * 1024 * 1024) {
      showDashboardToast('Attachment exceeds 4 MB');
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
    showDashboardToast(`Attached ${file.name}`);
  }

  onMount(() => { void loadModels(); });
  onDestroy(() => { streamController?.abort(); });
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>Chat</h1>
    <p>Test models through the gateway before wiring them into production.</p>
  </div>
  <div class="heading-actions">
    <button class="action-button" type="button" data-open-modal="chat-settings-dialog">
      <span class="button-icon"><svg class="icon"><use href="#i-sliders" /></svg></span>
      <span class="button-label">Parameters</span>
    </button>
    <button class="action-button primary" type="button" onclick={newChat}>
      <span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span>
      <span class="button-label">New chat</span>
    </button>
  </div>
</div>

<div class="page-surface chat-surface">
  <div class="chat-layout surface">
    <aside class="chat-side">
      <button class="chat-new-btn" type="button" onclick={newChat}>
        <svg class="icon"><use href="#i-plus" /></svg>
        New conversation
      </button>
      <div class="chat-sessions">
        {#each sessions as session (session.id)}
          <button class="chat-session" class:active={session.id === activeSessionId} type="button" onclick={() => selectSession(session.id)}>
            <strong>{session.title}</strong>
            <span>{session.modelLabel || 'no model'} · {session.messages.length} messages</span>
          </button>
        {:else}
          <p class="card-subtitle" style="padding:10px 4px">No conversations yet.</p>
        {/each}
      </div>
    </aside>

    <section class="chat-main">
      <div class="chat-model-bar">
        <span class="chat-model-label">Model</span>
        <select class="chat-model-select" aria-label="Select model" bind:value={selectedModel} disabled={streaming}>
          {#each modelOptions as option (option.id)}
            <option value={option.id}>{modelLabel(option)}</option>
          {/each}
        </select>
        {#if pendingAttachments.length}
          <span class="chat-via">{pendingAttachments.length} attachment{pendingAttachments.length > 1 ? 's' : ''} queued</span>
        {/if}
      </div>

      <div class="chat-messages" bind:this={messagesEl} aria-live="polite">
        {#if messages.length === 0}
          <div class="chat-empty">
            <svg class="icon chat-empty-icon"><use href="#i-video" /></svg>
            <strong>Start a conversation</strong>
            <p>Messages are routed through the gateway exactly like production traffic.</p>
            <div class="chat-suggestions">
              <button class="chat-suggestion" type="button" onclick={() => void send('Summarize this text in 3 bullets: ExoRoute is a protocol gateway that normalizes client and provider formats, adds retries, combos, quota tracking, and per-key scoping.')}>Summarize this text in 3 bullets</button>
              <button class="chat-suggestion" type="button" onclick={() => void send('Write a Python retry decorator with exponential backoff')}>Write a Python retry decorator</button>
              <button class="chat-suggestion" type="button" onclick={() => void send('Explain SSE vs WebSocket for LLM streaming')}>Explain SSE vs WebSocket</button>
            </div>
          </div>
        {/if}
        {#each messages as message (message.id)}
          <div class="chat-msg {message.role}" class:error={message.error}>
            <div class="chat-bubble" class:streaming={message.streaming}>{message.text}{#if message.streaming}<span class="chat-cursor"></span>{/if}</div>
          </div>
        {/each}
      </div>

      <form class="chat-composer" onsubmit={(e) => { e.preventDefault(); void send(); }}>
        <button class="icon-action" type="button" title="Attach image" onclick={() => fileInput.click()}>
          <svg class="icon"><use href="#i-paperclip" /></svg>
        </button>
        <input bind:this={fileInput} type="file" accept="image/*" hidden onchange={onAttach} />
        <textarea
          bind:this={inputEl}
          bind:value={composerText}
          rows="1"
          placeholder="Message the model… (Enter to send, Shift+Enter for newline)"
          aria-label="Message input"
          onkeydown={onKeydown}
          oninput={autoGrow}
          disabled={streaming}
        ></textarea>
        {#if streaming}
          <button class="chat-stop" type="button" title="Stop generating" onclick={stop}>
            <svg class="icon"><use href="#i-stop" /></svg>
          </button>
        {:else}
          <button class="chat-send" type="submit" title="Send" disabled={!composerText.trim() || !selectedModel}>
            <svg class="icon"><use href="#i-send" /></svg>
          </button>
        {/if}
      </form>
    </section>
  </div>
</div>

<!-- Chat parameters dialog -->
<dialog class="modal" bind:this={paramsDialog} aria-labelledby="chat-settings-title">
  <form class="modal-card" onsubmit={(e) => { e.preventDefault(); paramsDialog?.close(); showDashboardToast('Parameters applied to the next request'); }}>
    <div class="modal-heading">
      <div>
        <h2 id="chat-settings-title">Chat parameters</h2>
        <p>Applied to the next request through the gateway.</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="modal-fields chat-settings-fields">
      <label class="field">
        Temperature
        <input type="range" min="0" max="2" step="0.1" bind:value={temperature} />
        <span class="range-value">{temperature.toFixed(1)}</span>
      </label>
      <label class="field">
        Max tokens
        <input type="number" min="64" max="128000" step="64" bind:value={maxTokens} />
      </label>
      <label class="field prov-field-wide">
        System prompt
        <textarea rows="3" placeholder="You are a helpful routing assistant…" bind:value={systemPrompt}></textarea>
      </label>
      <label class="field chat-check">
        <input type="checkbox" bind:checked={streamEnabled} />
        Stream response (SSE)
      </label>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Cancel</button>
      <button class="page-action primary" type="submit">Apply</button>
    </div>
  </form>
</dialog>
