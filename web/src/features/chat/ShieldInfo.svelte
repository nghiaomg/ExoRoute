<script lang="ts">
  import { Lock } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';

  export let tr: Translate;
</script>

<div class="chat-privacy-note" role="note">
  <Lock size={14} />
  <span>{tr('Workspace chat runs through the gateway relay without request logging, telemetry, or usage metering. Closing this page discards the conversation.')}</span>
</div>

<style>
  /* Soft Neo-Brutalism notice: 2px ink border + hard offset shadow. */
  .chat-privacy-note {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 16px;
    padding: 10px 12px;
    border: 2px solid var(--ink);
    border-radius: 12px;
    box-shadow: 3px 3px 0 var(--ink);
    background: #fff7ed;
    color: var(--ink);
    font-size: 12px;
    font-weight: var(--font-medium);
  }

  :global(:root[data-theme='dark']) .chat-privacy-note { background: rgba(249, 115, 22, 0.08); }
</style>
