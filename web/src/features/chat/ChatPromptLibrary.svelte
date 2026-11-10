<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { Library, Save, Trash2, X } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';
  import Input from '../../components/Input.svelte';
  import {
    loadPrompts,
    savePrompts,
    makePrompt,
    type SavedPrompt,
  } from './chat.state';

  export let tr: Translate;
  /** The current system prompt text, prefilled when saving. */
  export let systemPrompt: string;
  /** Applies a saved prompt as the composer's system prompt. */
  export let onApply: (body: string) => void;
  export let onClose: () => void;

  let prompts: SavedPrompt[] = loadPrompts();
  let name = '';
  let savedFlash = false;

  function persist(next: SavedPrompt[]): void {
    prompts = next;
    savePrompts(next);
  }

  function saveCurrent(): void {
    const body = systemPrompt.trim();
    if (!body) return;
    const prompt = makePrompt(name || 'Prompt', body);
    persist([prompt, ...prompts].slice(0, 200));
    name = '';
    savedFlash = true;
    window.setTimeout(() => (savedFlash = false), 1_500);
  }

  function apply(prompt: SavedPrompt): void {
    onApply(prompt.body);
    onClose();
  }

  function remove(prompt: SavedPrompt): void {
    persist(prompts.filter((entry) => entry.id !== prompt.id));
  }
</script>

<div class="prompt-library" role="dialog" aria-label={tr('Prompt library')}>
  <header class="library-header">
    <strong>{tr('Prompt library')}</strong>
    <Button class="library-close" type="button" onclick={onClose} title={tr('Close')}>
      <X size={14} />
    </Button>
  </header>

  <div class="library-save">
    <Input
      class="library-name"
      type="text"
      placeholder={tr('Prompt name')}
      bind:value={name}
      maxlength={120}
    />
    <Button
      class="library-save-button"
      type="button"
      disabled={!systemPrompt.trim()}
      onclick={saveCurrent}
    >
      <Save size={12} />
      {tr('Save current system prompt')}
    </Button>
  </div>
  {#if savedFlash}
    <p class="library-flash">{tr('Saved.')}</p>
  {/if}

  {#if !prompts.length}
    <p class="library-empty">{tr('No saved prompts yet.')}</p>
  {:else}
    <ul class="library-list">
      {#each prompts as prompt (prompt.id)}
        <li class="library-row">
          <div class="library-row-main">
            <span class="library-row-name">{prompt.name}</span>
            <span class="library-row-preview">{prompt.body.slice(0, 80)}{prompt.body.length > 80 ? '…' : ''}</span>
          </div>
          <div class="library-row-actions">
            <Button class="library-apply" type="button" onclick={() => apply(prompt)}>
              {tr('Apply')}
            </Button>
            <Button
              class="library-delete"
              type="button"
              onclick={() => remove(prompt)}
              title={tr('Delete prompt')}
            >
              <Trash2 size={12} />
            </Button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .prompt-library {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0 auto 12px;
    max-width: 760px;
    padding: 10px 12px;
    border: var(--nb-border);
    border-radius: 14px;
    background: var(--paper);
    box-shadow: var(--nb-shadow-lg);
    max-height: 300px;
    overflow-y: auto;
  }

  .library-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  :global(.library-close) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: var(--nb-border);
    border-radius: 8px;
    background: var(--paper);
    color: var(--ink);
    cursor: pointer;
  }

  .library-save {
    display: flex;
    gap: 6px;
  }

  :global(.library-name) {
    flex: 1 1 auto;
    min-width: 0;
    padding: 6px 9px;
    border: var(--nb-border);
    border-radius: 9px;
    background: var(--paper);
    color: var(--ink);
    font-size: 12px;
  }

  :global(.library-save-button) {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border: var(--nb-border);
    border-radius: 9px;
    background: var(--paper);
    color: var(--ink);
    font-size: 11px;
    font-weight: var(--font-semibold);
    cursor: pointer;
    box-shadow: var(--nb-shadow-sm);
  }

  :global(.library-save-button:disabled) {
    opacity: 0.45;
    cursor: not-allowed;
    box-shadow: none;
  }

  .library-flash {
    margin: 0;
    font-size: 11px;
    color: #15803d;
  }

  .library-empty {
    margin: 0;
    font-size: 11px;
    color: var(--muted);
  }

  .library-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .library-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 9px;
    border: var(--nb-border);
    border-radius: 10px;
  }

  .library-row-main {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .library-row-name {
    font-size: 12px;
    font-weight: var(--font-bold);
    color: var(--ink);
  }

  .library-row-preview {
    font-size: 10px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .library-row-actions {
    display: flex;
    gap: 5px;
  }

  :global(.library-apply) {
    padding: 4px 9px;
    border: var(--nb-border);
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 14%, #fff);
    color: var(--ink);
    font-size: 11px;
    font-weight: var(--font-semibold);
    cursor: pointer;
  }

  :global(.library-delete) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: var(--nb-border);
    border-radius: 8px;
    background: var(--paper);
    color: #b91c1c;
    cursor: pointer;
  }

  :global(:root[data-theme='dark']) .prompt-library { background: color-mix(in srgb, var(--surface-glass-base) 90%, transparent); }
  :global(:root[data-theme='dark']) :global(.library-apply) { background: color-mix(in srgb, var(--accent) 20%, transparent); }
  :global(:root[data-theme='dark'] .library-name) {
    background: rgba(255, 255, 255, 0.04);
    color: #e2e8f0;
  }
</style>
