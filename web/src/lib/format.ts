import { getIntlLocale, type Locale } from './i18n';
import { isLiveRequest, type RequestLiveRow, type RequestLog } from './types';

export type Translate = (key: string, vars?: Record<string, string | number>) => string;

const dateFormatters = new Map<string, Intl.DateTimeFormat>();

export function formatFileSize(bytes: number): string {
  return bytes < 1024 * 1024
    ? `${Math.max(1, Math.round(bytes / 1024))} KB`
    : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatUptime(seconds: number, tr: Translate): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return days
    ? tr('{days}d {hours}h', { days, hours })
    : hours
      ? tr('{hours}h {minutes}m', { hours, minutes })
      : tr('{minutes}m', { minutes });
}

export function formatDate(value: string, locale: Locale): string {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return value;
  const dateLocale = getIntlLocale(locale);
  let formatter = dateFormatters.get(dateLocale);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(dateLocale, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
    dateFormatters.set(dateLocale, formatter);
  }
  return formatter.format(date);
}

export function formatGatewayEndpoint(
  host?: unknown,
  port?: unknown,
  windowHost?: string | null,
): string {
  if (windowHost && windowHost.trim()) {
    return `${windowHost.trim()}/v1`;
  }
  const h = typeof host === 'string' && host.trim() ? host.trim() : '127.0.0.1';
  const p = (typeof port === 'number' || typeof port === 'string') && String(port).trim()
    ? String(port).trim()
    : '8686';
  return `${h}:${p}/v1`;
}

const numberFormatters = new Map<string, Intl.NumberFormat>();

function numberFormatter(locale: Locale): Intl.NumberFormat {
  const key = getIntlLocale(locale);
  let formatter = numberFormatters.get(key);
  if (!formatter) {
    formatter = new Intl.NumberFormat(key);
    numberFormatters.set(key, formatter);
  }
  return formatter;
}

/// Timestamp a request row shows, ISO-encoded so `formatDate` can parse it.
export function requestCreatedAt(request: RequestLog | RequestLiveRow): string {
  return isLiveRequest(request) ? new Date(request.started_at_ms).toISOString() : request.created_at;
}

/// Numeric elapsed time for a row: live rows measure against the ticking clock,
/// a finished row uses the duration the gateway recorded. `null` means the row
/// does not report a duration (yet).
export function requestDurationMs(request: RequestLog | RequestLiveRow, liveClockMs: number): number | null {
  if (isLiveRequest(request)) return Math.max(0, liveClockMs - request.started_at_ms);
  return request.duration_ms != null && Number.isFinite(request.duration_ms) ? request.duration_ms : null;
}

/// Elapsed time for a row, or an em dash when the row has no duration.
export function requestDuration(request: RequestLog | RequestLiveRow, liveClockMs: number): string {
  const ms = requestDurationMs(request, liveClockMs);
  return ms == null ? '—' : `${ms} ms`;
}

/// Grouped digits for a dashboard counter, or an em dash when it is missing.
export function formatCount(value: number | null | undefined, locale: Locale): string {
  return value == null || !Number.isFinite(value) ? '—' : numberFormatter(locale).format(value);
}

/// Grouped digits for a token count, or an em dash when the gateway omitted it.
export function formatTokenCount(value: number | undefined, locale: Locale): string {
  return formatCount(value, locale);
}

const percentFormatters = new Map<string, Intl.NumberFormat>();

/// Whole-percent rendering of a 0–100 rate, or an em dash when it is unknown.
export function formatPercent(value: number | null | undefined, locale: Locale): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const key = getIntlLocale(locale);
  let formatter = percentFormatters.get(key);
  if (!formatter) {
    formatter = new Intl.NumberFormat(key, { style: 'percent', maximumFractionDigits: 0 });
    percentFormatters.set(key, formatter);
  }
  return formatter.format(value / 100);
}

/// Micro-USD cost as a dollar string, or an em dash when the gateway omitted it.
export function formatCostMicroUsd(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  if (value <= 0) return '$0.00';
  return `$${(value / 1_000_000).toFixed(6)}`;
}
