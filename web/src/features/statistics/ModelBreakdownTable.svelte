<script lang="ts">
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
  import type { ModelBreakdown } from '../../lib/types';

  export let items: ModelBreakdown[] = [];
  export let truncated = false;
  export let tr: Translate;
  export let locale: Locale;

  $: numberFormat = new Intl.NumberFormat(getIntlLocale(locale));

  function formatCount(value: number): string {
    return numberFormat.format(Number.isFinite(value) ? value : 0);
  }
</script>

<section class="model-breakdown-section" aria-label={tr('Models called')}>
  <div class="model-breakdown-heading">
    <div>
      <h2>{tr('Models called')}</h2>
      <p>{tr('Usage by model in the selected period.')}</p>
    </div>
  </div>

  {#if items.length}
    <div class="model-breakdown-card">
      <div class="model-breakdown-scroll">
        <table>
          <thead>
            <tr>
              <th>{tr('Model')}</th>
              <th>{tr('Combo')}</th>
              <th>{tr('Input tokens')}</th>
              <th>{tr('Output tokens')}</th>
              <th>{tr('Cache hit rate')}</th>
              <th>{tr('Requests')}</th>
              <th>{tr('Success rate')}</th>
            </tr>
          </thead>
          <tbody>
            {#each items as item (`${item.model}:${item.combo ?? ''}`)}
              <tr>
                <td class="model-cell" title={item.model}><code>{item.model}</code></td>
                <td class="combo-cell" title={item.combo ?? ''}>{item.combo ?? '—'}</td>
                <td class="number-cell">{formatCount(item.input_tokens)}</td>
                <td class="number-cell">{formatCount(item.output_tokens)}</td>
                <td class="number-cell">{item.cache_hit_rate.toFixed(1)}%</td>
                <td class="number-cell">{formatCount(item.requests)}</td>
                <td class="success-cell">
                  <strong>{item.success_rate.toFixed(1)}%</strong>
                  <small>{tr('{count} successful', { count: formatCount(item.successes) })}</small>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if truncated}
        <p class="model-breakdown-note">{tr('Model list is truncated after {count} entries.', { count: formatCount(items.length) })}</p>
      {/if}
    </div>
  {:else}
    <div class="model-breakdown-empty">{tr('No models called in the selected period.')}</div>
  {/if}
</section>

<style>
  .model-breakdown-section { margin-top: 14px; }
  .model-breakdown-heading { display: flex; align-items: end; justify-content: space-between; gap: 12px; margin-bottom: 10px; }
  .model-breakdown-heading h2 { margin: 0; color: var(--ink); font-size: 15px; }
  .model-breakdown-heading p { margin: 4px 0 0; color: var(--muted); font-size: 12px; }
  .model-breakdown-card { overflow: hidden; border: 1px solid var(--line); border-radius: 13px; background: var(--paper); }
  .model-breakdown-scroll { overflow-x: auto; }
  table { width: 100%; min-width: 760px; border-collapse: collapse; }
  th, td { padding: 12px 14px; border-bottom: 1px solid var(--line); text-align: left; white-space: nowrap; }
  th { color: var(--muted); font-size: 10px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; }
  td { color: var(--ink); font-size: 12px; }
  tbody tr:last-child td { border-bottom: 0; }
  .model-cell, .combo-cell { max-width: 220px; overflow: hidden; text-overflow: ellipsis; }
  .model-cell code { color: var(--ink); font: 600 12px var(--font-mono); }
  .combo-cell { color: var(--muted); }
  .number-cell { font-variant-numeric: tabular-nums; text-align: right; }
  .success-cell { min-width: 105px; }
  .success-cell strong, .success-cell small { display: block; }
  .success-cell strong { font: 600 12px var(--font-mono); }
  .success-cell small { margin-top: 3px; color: var(--muted); font-size: 11px; }
  .model-breakdown-note { margin: 0; padding: 10px 14px; color: var(--muted); border-top: 1px solid var(--line); font-size: 11px; }
  .model-breakdown-empty { padding: 26px 16px; color: var(--muted); border: 1px solid var(--line); border-radius: 13px; background: var(--paper); font-size: 12px; text-align: center; }
</style>
