/**
 * Shared shapes for the statistics chart components.
 *
 * These live outside the components because a chart's caller builds the rows and
 * its table twin is rendered by a different component, so both ends need the
 * same shape.
 */

import { formatPercent, type Translate } from '../../lib/format';
import type { Locale } from '../../lib/i18n';
import type { StatisticsDimension } from '../../lib/types';

/** One row of a ranked or ordered bar list. */
export interface BarRow {
  id: string;
  label: string;
  value: number;
  /** Formatted value written at the row's end. */
  display: string;
  /** Lines revealed on hover. */
  detail?: string[];
}

/** One column of a chart's table twin. */
export interface ChartColumn {
  label: string;
  numeric?: boolean;
}

/** Caller-supplied wording and formatting for `dimensionBars`. */
export interface DimensionBarOptions {
  tr: Translate;
  formatCount: (value: number) => string;
  locale: Locale;
  /** Label for the aggregated remainder row. */
  othersLabel: string;
}

/**
 * Rows for a bounded top-N list of a telemetry dimension, with the aggregated
 * remainder appended as its own row.
 *
 * The remainder is a real total for everything the top-N list left out, so it is
 * kept rather than dropped; it is labelled by the caller because "Others" is
 * copy, not a row identity a chart can invent.
 */
export function dimensionBars(
  items: StatisticsDimension[],
  others: number,
  total: number,
  options: DimensionBarOptions,
): BarRow[] {
  const { tr, formatCount, locale, othersLabel } = options;
  const rows = items.map((item) => ({
    id: `dimension\u0000${item.id}`,
    label: item.label,
    value: item.requests,
    display: formatCount(item.requests),
    detail: [
      `${tr('Requests')}: ${formatCount(item.requests)}`,
      `${tr('Share')}: ${formatPercent(item.percentage, locale)}`,
    ],
  }));
  if (others > 0) {
    rows.push({
      id: 'dimension\u0000others',
      label: othersLabel,
      value: others,
      display: formatCount(others),
      detail: [
        `${tr('Requests')}: ${formatCount(others)}`,
        `${tr('Share')}: ${formatPercent(total > 0 ? (others / total) * 100 : 0, locale)}`,
      ],
    });
  }
  return rows;
}
