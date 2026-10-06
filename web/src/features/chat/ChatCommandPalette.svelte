<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { Translate } from '../../lib/format';
  import { loadPrompts, type SavedPrompt } from './chat.state';

  export let tr: Translate;
  /** Applies `/clear`, `/continue`, `/compare` as composer instructions. */
  export let onCommand: (command: 'clear' | 'continue' | 'compare' | 'library' | 'export-md' | 'export-json' | 'print') => void;
  /** Applies a saved prompt as the system prompt. */
  export let onApplyPrompt: (prompt: SavedPrompt) => void;
  export let onClose: () => void;

  interface PaletteEntry {
    id: string;
    label: string;
    hint: string;
    kind: 'command' | 'prompt';
    run: () => void;
  }

  let query = '';
  let activeIndex = 0;
  let inputElement: HTMLInputElement | null = null;
  const prompts: SavedPrompt[] = loadPrompts();

  const commands: PaletteEntry[] = [
    { id: 'cmd-clear', key: 'clear' },
    { id: 'cmd-continue', key: 'continue' },
    { id: 'cmd-compare', key: 'compare' },
    { id: 'cmd-library', key: 'library' },
    { id: 'cmd-export-md', key: 'export-md' },
    { id: 'cmd-export-json', key: 'export-json' },
    { id: 'cmd-print', key: 'print' },
  ].map(({ id, key }) => ({
    id,
    label: `/${key}`,
    hint: {
      'clear': tr('Clear conversation'),
      'continue': tr('Continue generation'),
      'compare': tr('Compare models'),
      'library': tr('Prompt library'),
      'export-md': tr('Export conversation as Markdown'),
      'export-json': tr('Export conversation as JSON'),
      'print': tr('Print conversation'),
    }[key as string] ?? key,
    kind: 'command' as const,
    run: () => onCommand(key as 'clear'),
  }));

  const promptEntries: PaletteEntry[] = prompts.map((prompt) => ({
    id: prompt.id,
    label: prompt.name,
    hint: prompt.body.slice(0, 80),
    kind: 'prompt' as const,
    run: () => onApplyPrompt(prompt),
  }));

  $: entries = [...commands, ...promptEntries].filter((entry) => {
    const needle = query.trim().toLowerCase();
    if (!needle) return true;
    return (
      entry.label.toLowerCase().includes(needle) ||
      entry.hint.toLowerCase().includes(needle)
    );
  });

  $: activeIndex = Math.min(activeIndex, Math.max(0, entries.length - 1));

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      activeIndex = Math.min(activeIndex + 1, entries.length - 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      activeIndex = Math.max(activeIndex - 1, 0);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const entry = entries[activeIndex];
      if (entry) {
        entry.run();
        onClose();
      }
    } else if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
    }
  }

  onMount(() => {
    void tick().then(() => inputElement?.focus());
  });
</script>

<svelte:window on:keydown={(event) => {
  if (event.key === 'Escape') onClose();
}} />

<div class="palette-backdrop" on:click={onClose} role="presentation">
  <div
    class="palette"
    role="dialog"
    aria-label={tr('Commands')}
    tabindex="-1"
    on:click|stopPropagation
    on:keydown={handleKeydown}
  >
    <input
      class="palette-input"
      type="text"
      placeholder={tr('Search prompts and commands')}
      bind:value={query}
      bind:this={inputElement}
      maxlength={200}
    />
    {#if !entries.length}
      <p class="palette-empty">{tr('No saved prompts yet.')}</p>
    {:else}
      <ul class="palette-list" role="listbox">
        {#each entries as entry, index (entry.id)}
          <li>
            <button
              class="palette-entry"
              class:active={index === activeIndex}
              type="button"
              role="option"
              aria-selected={index === activeIndex}
              on:click={() => {
                entry.run();
                onClose();
              }}
              on:mouseenter={() => (activeIndex = index)}
            >
              <span class="palette-entry-kind">{entry.kind === 'command' ? tr('Commands') : tr('Prompt library')}</span>
              <span class="palette-entry-label">{entry.label}</span>
              <span class="palette-entry-hint">{entry.hint}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    <footer class="palette-footer">
      <span>↑ ↓ {tr('Search prompts and commands')}</span>
      <span>⏎ {tr('Apply')}</span>
      <span>esc {tr('Close')}</span>
    </footer>
  </div>
</div>

<style>
  .palette-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
    background: rgba(15, 23, 42, 0.4);
  }

  .palette {
    display: flex;
    flex-direction: column;
    width: min(560px, calc(100vw - 32px));
    border: 2px solid var(--ink);
    border-radius: 14px;
    background: var(--paper);
    box-shadow: 6px 6px 0 var(--ink);
    overflow: hidden;
  }

  .palette-input {
    padding: 12px 14px;
    border: none;
    border-bottom: 2px solid var(--ink);
    background: var(--paper);
    color: var(--ink);
    font-size: 14px;
    outline: none;
  }

  .palette-empty {
    margin: 0;
    padding: 14px;
    font-size: 12px;
    color: var(--muted);
  }

  .palette-list {
    list-style: none;
    margin: 0;
    padding: 6px;
    max-height: 320px;
    overflow-y: auto;
  }

  .palette-entry {
    display: grid;
    grid-template-columns: auto auto 1fr;
    gap: 8px;
    align-items: baseline;
    width: 100%;
    padding: 7px 10px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--ink);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }

  .palette-entry.active {
    background: #ffedd5;
  }

  .palette-entry-kind {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  .palette-entry-label {
    font-weight: var(--font-bold);
  }

  .palette-entry-hint {
    font-size: 10px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .palette-footer {
    display: flex;
    gap: 14px;
    padding: 7px 12px;
    border-top: 1.5px solid var(--ink);
    font-size: 10px;
    color: var(--muted);
  }

  :global(:root[data-theme='dark']) .palette { background: rgba(14, 16, 26, 0.97); color: #e2e8f0; }
  :global(:root[data-theme='dark']) .palette-entry.active { background: rgba(249, 115, 22, 0.18); }
  :global(:root[data-theme='dark']) .palette-input {
    background: transparent;
    color: #e2e8f0;
  }
</style>
