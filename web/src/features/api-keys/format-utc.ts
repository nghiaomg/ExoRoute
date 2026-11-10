import { getIntlLocale, type Locale } from '../../lib/i18n';

/**
 * Formats a stored `YYYY-MM-DD HH:MM:SS` UTC timestamp for display.
 *
 * The database stores UTC without a zone suffix, so the string is tagged as UTC
 * before parsing; passing it to `Date` as-is would read it as local time and
 * shift every displayed instant by the viewer's offset.
 */
export function formatUtcTimestamp(value: string | null | undefined, locale: Locale): string | null {
  if (!value) return null;
  const parsed = new Date(`${value.replace(' ', 'T')}Z`);
  if (Number.isNaN(parsed.valueOf())) return null;
  return new Intl.DateTimeFormat(getIntlLocale(locale), {
    dateStyle: 'medium',
    timeStyle: 'short',
    timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  }).format(parsed);
}
