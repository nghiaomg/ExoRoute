<script lang="ts">
  import type { Translate } from '../../lib/format';

  /** Inline feedback under the composer: a session notice, a failed turn, or
   * the model list being unavailable. Only the model list is recoverable in
   * place, so it is the one variant that offers a retry. */
  export let tr: Translate;
  export let message: string;
  export let tone: 'error' | 'notice' | 'model' = 'notice';
  export let onRetry: (() => void) | null = null;
</script>

{#if tone === 'model'}
  <div class="chat-notice model" role="alert">
    <span>{message}</span>
    {#if onRetry}
      <button class="model-retry" type="button" onclick={onRetry}>{tr('Retry')}</button>
    {/if}
  </div>
{:else}
  <div class="chat-notice" class:error={tone === 'error'} role={tone === 'error' ? 'alert' : 'status'}>
    {message}
  </div>
{/if}

<style>
  .chat-notice {
    margin: 10px auto 0;
    max-width: 760px;
    padding: 8px 12px;
    border: 2px dashed var(--ink);
    border-radius: 12px;
    color: var(--muted);
    font-size: 12px;
  }

  .chat-notice.error {
    border: 2px solid #b91c1c;
    box-shadow: 3px 3px 0 #b91c1c;
    background: #fef2f2;
    color: #b91c1c;
    font-weight: var(--font-semibold);
  }

  .chat-notice.model {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    border: 2px solid #b45309;
    box-shadow: 3px 3px 0 #b45309;
    background: #fffbeb;
    color: #92400e;
    font-weight: var(--font-semibold);
  }

  .model-retry {
    flex: 0 0 auto;
    padding: 4px 12px;
    border: 2px solid var(--ink);
    border-radius: 9px;
    background: var(--paper);
    box-shadow: 2px 2px 0 var(--ink);
    color: var(--ink);
    font-size: 11px;
    font-weight: var(--font-semibold);
    cursor: pointer;
    transition: transform 0.12s ease, box-shadow 0.12s ease;
  }

  .model-retry:hover {
    transform: translate(-1px, -1px);
    box-shadow: 3px 3px 0 var(--ink);
  }

  .model-retry:active {
    transform: translate(1px, 1px);
    box-shadow: 1px 1px 0 var(--ink);
  }

  :global(:root[data-theme='dark']) .chat-notice.error {
    border-color: #f87171;
    box-shadow: 3px 3px 0 #f87171;
    background: #2b1818;
    color: #fecaca;
  }

  :global(:root[data-theme='dark']) .chat-notice.model {
    background: rgba(251, 191, 36, 0.12);
    color: #fde68a;
  }
</style>
