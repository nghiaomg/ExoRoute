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

/// Elapsed time for a row: live rows measure against the ticking clock, a
/// finished row uses the duration the gateway recorded.
export function requestDuration(request: RequestLog | RequestLiveRow, liveClockMs: number): string {
  if (isLiveRequest(request)) return `${Math.max(0, liveClockMs - request.started_at_ms)} ms`;
  return request.duration_ms != null ? `${request.duration_ms} ms` : '—';
}

/// Grouped digits for a token count, or an em dash when the gateway omitted it.
export function formatTokenCount(value: number | undefined, locale: Locale): string {
  return value == null || !Number.isFinite(value) ? '—' : numberFormatter(locale).format(value);
}
