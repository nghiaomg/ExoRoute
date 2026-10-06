<script lang="ts">
  import type { Locale } from '../../lib/i18n';
  import { formatTokenCount, type Translate } from '../../lib/format';

  interface Props {
    tr: Translate;
    locale: Locale;
    /** Cumulative session usage reported by finished turns. */
    tokens: { input: number; output: number };
    /** Operator-priced estimate; null when the model has no price row. */
    cost: { currency: string; amount: number } | null;
    historySeverity: 'ok' | 'near' | 'over';
    historyTurns: number;
    maxHistoryTurns: number;
  }

  let {
    tr,
    locale,
    tokens,
    cost,
    historySeverity,
    historyTurns,
    maxHistoryTurns,
  }: Props = $props();
</script>

<span class="chat-topbar-note" role="status">
  {tr('{input} in / {output} out', {
    input: formatTokenCount(tokens.input, locale),
    output: formatTokenCount(tokens.output, locale),
  })}
</span>
{#if cost}
  <span class="chat-topbar-note" role="status" title={tr('Session tokens')}>
    {cost.currency}{cost.amount.toFixed(4)}
  </span>
{/if}
<span
  class:near={historySeverity !== 'ok'}
  class="chat-topbar-note"
  role="status"
  title={historySeverity === 'ok'
    ? tr('Session tokens')
    : tr('History is near the context budget; older turns are dropped on send.')}
>
  {tr('History: {turns}/{maxTurns} turns', {
    turns: historyTurns,
    maxTurns: maxHistoryTurns,
  })}
</span>

<style>
  .chat-topbar-note {
    font-size: 11px;
    color: var(--muted);
  }

  .chat-topbar-note.near {
    padding: 2px 7px;
    border: 2px solid #b45309;
    border-radius: 999px;
    background: #fff7ed;
    color: #b45309;
    font-weight: var(--font-semibold);
  }

  :global(:root[data-theme='dark']) .chat-topbar-note.near {
    background: rgba(249, 115, 22, 0.14);
    border-color: #fdba74;
    color: #fdba74;
  }
</style>
