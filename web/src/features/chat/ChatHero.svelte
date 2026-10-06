<script lang="ts">
  import ShieldInfo from './ShieldInfo.svelte';
  import type { Translate } from '../../lib/format';

  interface Props {
    tr: Translate;
    /** One suggestion per card; applying fills the composer. */
    suggestions: Array<{ title: string; body: string }>;
    onApplySuggestion: (body: string) => void;
  }

  let { tr, suggestions, onApplySuggestion }: Props = $props();
</script>

<div class="chat-hero">
  <ShieldInfo {tr} />
  <h2 class="chat-hero-title">{tr('Workspace chat')}</h2>
  <p class="chat-hero-subtitle">
    {tr('Talk to one saved model. Nothing here is written to the database.')}
  </p>
  <div class="chat-hero-suggestions">
    {#each suggestions as suggestion (suggestion.title)}
      <button class="chat-suggestion" type="button" onclick={() => onApplySuggestion(suggestion.body)}>
        <span class="chat-suggestion-title">{suggestion.title}</span>
        <span class="chat-suggestion-body">{suggestion.body}</span>
      </button>
    {/each}
  </div>
</div>

<style>
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

  @media (max-width: 650px) {
    .chat-hero-suggestions {
      grid-template-columns: 1fr;
    }
  }
</style>
