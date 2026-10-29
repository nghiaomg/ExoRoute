<script lang="ts">
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
  import type { RequestStatistics } from '../../lib/types';

  export let tr: Translate;
  export let locale: Locale;
  export let statistics: RequestStatistics;

  $: numberFormat = new Intl.NumberFormat(getIntlLocale(locale));

  function formatCount(value: number): string {
    return numberFormat.format(Number.isFinite(value) ? value : 0);
  }

  function formatTimestamp(value: string | null): string {
    if (!value) return tr('Not available');
    const parsed = new Date(`${value.replace(' ', 'T')}Z`);
    if (Number.isNaN(parsed.getTime())) return value;
    return new Intl.DateTimeFormat(getIntlLocale(locale), {
      dateStyle: 'medium', timeStyle: 'short', timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    }).format(parsed);
  }
</script>

<section class="statistics-details">
  <div><span>{tr('Input tokens')}</span><strong>{formatCount(statistics.input_tokens)}</strong></div>
  <div><span>{tr('Output tokens')}</span><strong>{formatCount(statistics.output_tokens)}</strong></div>
  <div><span>{tr('Cached tokens')}</span><strong>{formatCount(statistics.cached_tokens)}</strong></div>
  <div><span>{tr('Cache ratio')}</span><strong>{statistics.cache_ratio.toFixed(1)}%</strong></div>
  <div><span>{tr('Data collected since')}</span><strong>{formatTimestamp(statistics.collection_started_at)}</strong></div>
  <div><span>{tr('Last aggregate update')}</span><strong>{formatTimestamp(statistics.as_of)}</strong></div>
</section>

<style>
  .statistics-details { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 1px; overflow: hidden; border: 2px solid var(--ink); border-radius: 13px; background: var(--line); box-shadow: 3px 3px 0 var(--ink); margin-top: 14px; }
  .statistics-details > div { min-width: 0; display: grid; gap: 8px; padding: 15px 17px; background: var(--paper); }
  .statistics-details span { color: var(--muted); font-size: 12px; }
  .statistics-details strong { overflow: hidden; color: var(--ink); font: 600 12px var(--font-sans); text-overflow: ellipsis; white-space: nowrap; }
  @media (max-width: 1000px) { .statistics-details { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @media (max-width: 620px) { .statistics-details { grid-template-columns: 1fr 1fr; } }
</style>
