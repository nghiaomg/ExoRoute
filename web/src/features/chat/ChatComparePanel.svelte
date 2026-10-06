<script lang="ts">
  import { onDestroy } from 'svelte';
  import { X, Square } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';
  import type { WorkspaceChatInput, WorkspaceChatModelOption } from '../../lib/types';
  import { streamWorkspaceChat } from '../../lib/api/workspace-chat-stream';
  import { localizedError } from '../../lib/errors';
  import { emptyCompareColumn, MAX_COMPARE_MODELS, type CompareColumn } from './chat.state';
  import { renderMarkdown } from './markdown';

  export let tr: Translate;
  export let options: WorkspaceChatModelOption[];
  /** The exact input the main composer validated, reused per column. */
  export let input: WorkspaceChatInput;
  export let onClose: () => void;

  let selected: string[] = [];
  let columns: CompareColumn[] = [];
  let controllers: AbortController[] = [];

  $: chosen = options.filter((option) => selected.includes(option.id));

  function toggleModel(modelId: string): void {
    if (selected.includes(modelId)) {
      selected = selected.filter((id) => id !== modelId);
      return;
    }
    if (selected.length >= MAX_COMPARE_MODELS) return;
    selected = [...selected, modelId];
  }

  async function startCompare(): Promise<void> {
    if (chosen.length < 2 || columns.length) return;
    const baseInput = input;
    columns = chosen.map((option) => emptyCompareColumn(option.id));
    for (let index = 0; index < columns.length; index += 1) {
      const column = columns[index];
      const controller = new AbortController();
      controllers.push(controller);
      const chosenOption = chosen[index];
      // The relay takes the saved provider id, not the model-id alias prefix.
      const columnInput: WorkspaceChatInput = {
        ...baseInput,
        provider_id: chosenOption.provider_id || optionProviderId(chosenOption.id),
        model: chosenOption.model || optionModelName(chosenOption.id),
      };
      void streamWorkspaceChat(
        columnInput,
        {
          onTextDelta: (delta) => {
            column.text += delta;
            columns = columns;
          },
          onReasoningDelta: (delta) => {
            column.reasoning += delta;
          },
          onDone: (done) => {
            column.state = 'done';
            column.duration_ms = done.duration_ms;
            if (done.input_tokens !== null || done.output_tokens !== null) {
              column.usage = {
                input_tokens: done.input_tokens ?? 0,
                output_tokens: done.output_tokens ?? 0,
              };
            }
            columns = columns;
          },
          onError: (message) => {
            if (controller.signal.aborted) {
              column.state = 'stopped';
              column.detail = message;
            } else {
              column.state = 'error';
              column.detail = message;
            }
            columns = columns;
          },
        },
        controller.signal,
      ).catch((error: unknown) => {
        if (column.state !== 'running') return;
        // A stop the operator asked for is not a failure; every other
        // rejection carries the relay's own reason (missing credential,
        // unknown provider, expired session) instead of a generic message.
        const stopped = controller.signal.aborted;
        column.state = stopped ? 'stopped' : 'error';
        column.detail = stopped
          ? ''
          : localizedError(error, 'The chat stream ended unexpectedly.', tr);
        columns = columns;
      });
    }
  }

  function optionProviderId(modelId: string): string {
    return modelId.split('/')[0] ?? '';
  }

  function optionModelName(modelId: string): string {
    const [, ...rest] = modelId.split('/');
    return rest.join('/');
  }

  function stopCompare(): void {
    for (const controller of controllers) controller.abort();
  }

  function close(): void {
    stopCompare();
    onClose();
  }

  onDestroy(stopCompare);

  function markdownFor(column: CompareColumn): string {
    return renderMarkdown(column.text);
  }
</script>

<div class="compare-panel" role="dialog" aria-label={tr('Compare models')}>
  <header class="compare-header">
    <strong>{tr('Compare models')}</strong>
    <span class="compare-warning">{tr('Each model runs a real, billed request in parallel.')}</span>
    <div class="compare-header-actions">
      {#if columns.length}
        <button class="compare-stop" type="button" on:click={stopCompare}>
          <Square size={12} />
          {tr('Stop')}
        </button>
      {/if}
      <button class="compare-close" type="button" on:click={close} title={tr('Close')}>
        <X size={14} />
      </button>
    </div>
  </header>

  {#if !columns.length}
    <div class="compare-picker">
      <p class="compare-hint">
        {tr('Pick two to four models to compare on this exact prompt.')}
      </p>
      <div class="compare-options">
        {#each options as option (option.id)}
          <button
            class="compare-option"
            class:active={selected.includes(option.id)}
            type="button"
            disabled={!selected.includes(option.id) && selected.length >= MAX_COMPARE_MODELS}
            on:click={() => toggleModel(option.id)}
          >
            {option.id}
          </button>
        {/each}
      </div>
      <button
        class="compare-start"
        type="button"
        disabled={chosen.length < 2}
        on:click={startCompare}
      >
        {tr('Run comparison')}
      </button>
    </div>
  {:else}
    <div class="compare-grid" style="--compare-cols: {columns.length}">
      {#each columns as column (column.modelId)}
        <section class="compare-column">
          <header class="compare-column-header">
            <span class="compare-model">{column.modelId}</span>
            <span class="compare-state" data-state={column.state}>
              {column.state === 'running'
                ? tr('Streaming…')
                : column.state === 'done'
                  ? tr('Done')
                  : column.state === 'stopped'
                    ? tr('Generation stopped.')
                    : tr('Failed')}
            </span>
          </header>
          {#if column.detail}
            <p class="compare-error">{column.detail}</p>
          {/if}
          <div class="compare-text">
            {#if column.text}
              {@html renderMarkdown(column.text)}
            {:else}
              <p class="compare-empty">{column.state === 'running' ? '…' : '—'}</p>
            {/if}
          </div>
          {#if column.usage}
            <footer class="compare-meta">
              {column.usage.input_tokens} → {column.usage.output_tokens} tok
              {#if column.duration_ms}
                · {column.duration_ms} ms
              {/if}
            </footer>
          {/if}
        </section>
      {/each}
    </div>
  {/if}
</div>

<style>
  .compare-panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0 auto 12px;
    max-width: 1100px;
    padding: 10px 12px;
    border: 2px solid var(--ink);
    border-radius: 14px;
    background: var(--paper);
    box-shadow: 4px 4px 0 var(--ink);
  }

  .compare-header {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .compare-warning {
    font-size: 10px;
    color: #b45309;
    font-weight: var(--font-semibold);
  }

  .compare-header-actions {
    margin-left: auto;
    display: flex;
    gap: 6px;
  }

  .compare-stop,
  .compare-close,
  .compare-start {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border: 2px solid var(--ink);
    border-radius: 9px;
    background: var(--paper);
    color: var(--ink);
    font-size: 11px;
    font-weight: var(--font-semibold);
    cursor: pointer;
    box-shadow: 2px 2px 0 var(--ink);
  }

  .compare-stop:disabled,
  .compare-start:disabled {
    opacity: 0.45;
    cursor: not-allowed;
    box-shadow: none;
  }

  .compare-picker {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .compare-hint {
    margin: 0;
    font-size: 11px;
    color: var(--muted);
  }

  .compare-options {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    max-height: 130px;
    overflow-y: auto;
  }

  .compare-option {
    padding: 4px 9px;
    border: 1.5px solid var(--ink);
    border-radius: 999px;
    background: var(--paper);
    color: var(--ink);
    font-size: 10px;
    cursor: pointer;
  }

  .compare-option.active {
    background: #ffedd5;
    font-weight: var(--font-bold);
  }

  .compare-option:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .compare-grid {
    display: grid;
    grid-template-columns: repeat(var(--compare-cols, 2), minmax(0, 1fr));
    gap: 10px;
  }

  .compare-column {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    border: 2px solid var(--ink);
    border-radius: 10px;
    min-height: 120px;
    max-height: 340px;
    overflow-y: auto;
  }

  .compare-column-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .compare-model {
    font-size: 11px;
    font-weight: var(--font-bold);
    color: var(--ink);
  }

  .compare-state {
    font-size: 10px;
    color: var(--muted);
  }

  .compare-state[data-state='error'] {
    color: #b91c1c;
    font-weight: var(--font-semibold);
  }

  .compare-error {
    margin: 0;
    font-size: 10px;
    color: #b91c1c;
  }

  .compare-text {
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }

  .compare-empty {
    margin: 0;
    color: var(--muted);
  }

  .compare-meta {
    font-size: 10px;
    color: var(--muted);
  }

  :global(:root[data-theme='dark']) .compare-panel { background: rgba(14, 16, 26, 0.9); }
  :global(:root[data-theme='dark']) .compare-column { border-color: rgba(255, 255, 255, 0.25); }
  :global(:root[data-theme='dark']) .compare-option.active { background: rgba(249, 115, 22, 0.2); }
</style>
