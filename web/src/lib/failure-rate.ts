import { api } from './api';
import { getIntlLocale, t, type Locale } from './i18n';
import type { RequestStatistics } from './types';

/**
 * Failure-rate alert for the dashboard shell.
 *
 * Once per interval the shell reuses the admin statistics snapshot the
 * Requests page already trusts and compares its 24-hour success rate
 * against a floor. The same best-effort contract as release-check.ts:
 * every failure path returns null, the banner only appears for a real
 * incident, and dismissal is remembered per incident so a fresh incident
 * re-notifies.
 */

export const FAILURE_RATE_THRESHOLD = 0.9;
export const FAILURE_RATE_MIN_REQUESTS = 20;
export const FAILURE_RATE_RANGE: RequestStatistics['range'] = '1d';
export const FAILURE_RATE_POLL_MS = 60_000;

const DISMISSED_STORAGE_KEY = 'exoroute.failure-alert-dismissed';

export interface FailureAlert {
  /** Snapshot marker identifying the incident; also the dismissal unit. */
  incidentId: string;
  successRate: number;
  failures: number;
  total: number;
}

/** Pure threshold predicate: small samples never raise the alert. */
export function shouldAlertFailureRate(statistics: Pick<RequestStatistics, 'success_rate' | 'total_requests'>): boolean {
  if (!Number.isFinite(statistics.success_rate) || !Number.isFinite(statistics.total_requests)) return false;
  if (statistics.total_requests < FAILURE_RATE_MIN_REQUESTS) return false;
  return statistics.success_rate < FAILURE_RATE_THRESHOLD;
}

function readDismissed(): string | null {
  try {
    const raw = localStorage.getItem(DISMISSED_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as unknown;
    return typeof parsed === 'string' && parsed.length > 0 ? parsed : null;
  } catch {
    return null;
  }
}

/** Remembers which incident window the user dismissed. */
export function dismissFailureAlert(incidentId: string): void {
  try {
    localStorage.setItem(DISMISSED_STORAGE_KEY, JSON.stringify(incidentId));
  } catch {}
}

export interface FailureCheckOptions {
  stats?: () => Promise<RequestStatistics>;
}

/**
 * Fetches the statistics snapshot (or reuses a provided one, for tests) and
 * returns the incident to show, or null when the rate is healthy, the
 * sample is too small, the incident is dismissed, or anything fails.
 */
export async function checkFailureRate(options: FailureCheckOptions = {}): Promise<FailureAlert | null> {
  let statistics: RequestStatistics;
  try {
    statistics = options.stats ? await options.stats() : await api.statistics(FAILURE_RATE_RANGE);
  } catch {
    return null;
  }
  if (!shouldAlertFailureRate(statistics)) return null;
  if (readDismissed() === statistics.as_of) return null;
  return {
    incidentId: statistics.as_of,
    successRate: statistics.success_rate,
    failures: statistics.failures,
    total: statistics.total_requests,
  };
}

/** Formats the success rate for the banner in the current display locale. */
export function formatFailureRate(alert: FailureAlert, locale: Locale): string {
  return new Intl.NumberFormat(getIntlLocale(locale), {
    style: 'percent',
    maximumFractionDigits: 0,
  }).format(alert.successRate);
}

export function failureAlertMessage(alert: FailureAlert, locale: Locale): string {
  return t(
    locale,
    'Failure rate is {rate} over the last day ({failures} of {total} requests).',
    {
      rate: formatFailureRate(alert, locale),
      failures: new Intl.NumberFormat(getIntlLocale(locale)).format(alert.failures),
      total: new Intl.NumberFormat(getIntlLocale(locale)).format(alert.total),
    },
  );
}
