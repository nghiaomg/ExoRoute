<script lang="ts">
  import { formatCostMicroUsd, type Translate } from '../../lib/format';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import type { ProviderBreakdown, RequestStatistics } from '../../lib/types';
  import ChartFrame from './ChartFrame.svelte';
  import ChartTableView from './ChartTableView.svelte';
  import RankedBars from '../../components/RankedBars.svelte';
  import type { BarRow } from '../../components/chartTypes';

  /**
   * Spend for the period: one headline total and the same total split by the
   * provider that served the request.
   *
   * Cost is reported per request and may be missing for some of them. A group is
   * only shown as a total when every request in it reported a cost; otherwise the
   * reported sum is labelled a lower bound and the panel carries the partial
   * badge. That keeps an unreported cost from reading as a zero-cost request.
   */
  export let statistics: RequestStatistics;
  export let locale: Locale;
  export let tr: Translate;
  export let formatCount: (value: number) => string;

  /** One ranked row, kept beside the provider record it came from. */
  interface SpendRow {
    bar: BarRow;
    provider: ProviderBreakdown;
  }

  $: intlLocale = getIntlLocale(locale);
  $: msFormat = new Intl.NumberFormat(intlLocale, { maximumFractionDigits: 1 });
  // A period with no requests has no cost total at all, which is empty rather
  // than partial: only a group that reported some costs can be a lower bound.
  $: totalPartial = statistics.cost.requests > 0 && statistics.cost.cost_micro_usd === null;
  $: totalValue = statistics.cost.cost_micro_usd ?? statistics.cost.cost_reported_micro_usd;
  $: totalLabel = totalPartial
    ? tr('{cost} or more', { cost: formatCostMicroUsd(totalValue) })
    : formatCostMicroUsd(totalValue);
  $: spendRows = buildRows();
  $: bars = spendRows.map((entry) => entry.bar);

  function providerName(row: ProviderBreakdown): string {
    return row.provider ?? tr('No provider selected');
  }

  function costLabel(row: ProviderBreakdown): string {
    return row.missing_cost_events > 0
      ? tr('{cost} or more', { cost: formatCostMicroUsd(row.cost_reported_micro_usd) })
      : formatCostMicroUsd(row.cost_micro_usd);
  }

  function buildRows(): SpendRow[] {
    return statistics.providers.rows
      .filter((row) => row.cost_reported_micro_usd > 0)
      .sort((left, right) => right.cost_reported_micro_usd - left.cost_reported_micro_usd)
      .map((provider) => ({
        provider,
        bar: {
          id: `cost\u0000${provider.provider ?? ''}`,
          label: providerName(provider),
          // Reported cost is always present; the exact total may not be.
          value: provider.cost_micro_usd ?? provider.cost_reported_micro_usd,
          display: costLabel(provider),
          detail: [
            `${tr('Requests')}: ${formatCount(provider.requests)}`,
            `${tr('Cost')}: ${costLabel(provider)}`,
            `${tr('Average latency')}: ${msFormat.format(provider.average_latency_ms)} ms`,
          ],
        },
      }));
  }
</script>

<ChartFrame
  title={tr('Spend')}
  {tr}
  caption={tr('Reported cost across the retained request history for this period.')}
  truncated={statistics.history_truncated}
  partial={totalPartial}
  empty={spendRows.length === 0}
  emptyText={statistics.cost.requests > 0
    ? tr('No request in the selected period reported a cost.')
    : tr('No requests in the selected period.')}
  partialText={tr('Some requests in this period reported no cost, so the total and each bar are lower bounds.')}
>
  <svelte:fragment slot="chart">
    <p class="cost-total">
      <strong>{totalLabel}</strong>
      <span>{tr('Across {count} requests.', { count: formatCount(statistics.cost.requests) })}</span>
    </p>
    <h3 class="cost-subtitle">{tr('Cost by provider')}</h3>
    <RankedBars rows={bars} maxValue={Math.max(0, ...bars.map((bar) => bar.value))} {tr} />
  </svelte:fragment>
  <svelte:fragment slot="table">
    <ChartTableView
      columns={[
        { label: tr('Provider') },
        { label: tr('Requests'), numeric: true },
        { label: tr('Cost'), numeric: true },
        { label: tr('Average latency'), numeric: true },
      ]}
      rows={spendRows.map((entry) => [
        providerName(entry.provider),
        formatCount(entry.provider.requests),
        entry.bar.display,
        `${msFormat.format(entry.provider.average_latency_ms)} ms`,
      ])}
    />
  </svelte:fragment>
</ChartFrame>

<style>
  .cost-total { display: grid; gap: 3px; margin: 0 0 16px; }
  .cost-total strong { color: var(--ink); font: 700 26px/1.15 var(--font-mono); font-variant-numeric: tabular-nums; }
  .cost-total span { color: var(--muted); font-size: 12px; }
  .cost-subtitle { margin: 0 0 10px; color: var(--muted); font: 600 12px var(--font-sans); }
</style>
