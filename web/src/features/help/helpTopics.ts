import type { DashboardPage } from '../../lib/navigation';
import type { Translate } from '../../lib/format';

/** One troubleshooting topic shown in the Help Center FAQ accordion. */
export interface HelpTopic {
  id: string;
  /** Extra lowercase search terms that the visible copy does not contain. */
  keywords: string;
  title: string;
  body: string;
  /** Dashboard page that resolves the topic, when one exists. */
  target?: DashboardPage;
}

/** One documented gateway status code. */
export interface HelpErrorCode {
  status: number;
  /** HTTP reason phrase — a protocol constant, kept untranslated. */
  reason: string;
  detail: string;
  keywords: string;
}

/** Gateway status contract, matching the values `src/gateway` returns. */
const ERROR_REASONS: ReadonlyArray<{ status: number; reason: string; keywords: string }> = [
  { status: 401, reason: 'Unauthorized', keywords: 'auth authentication key invalid missing' },
  { status: 403, reason: 'Forbidden', keywords: 'scope provider model restriction blocked' },
  { status: 404, reason: 'Not found', keywords: 'model route alias missing unknown' },
  { status: 429, reason: 'Too many requests', keywords: 'rate limit quota retry-after throttled' },
  { status: 502, reason: 'Bad gateway', keywords: 'upstream provider failed rejected attempt' },
  { status: 503, reason: 'Service unavailable', keywords: 'storage busy disabled shutting down' },
  { status: 504, reason: 'Gateway timeout', keywords: 'timeout deadline slow expired' },
];

/** Troubleshooting topics, in the order the Help Center lists them. */
export function faqTopics(tr: Translate): HelpTopic[] {
  return [
    {
      id: 'failover',
      keywords: 'failover combo priority retry target rotation',
      title: tr('How does failover between providers work?'),
      body: tr('A combo tries its targets in priority order. Retryable upstream failures (429, 5xx, timeouts) move to the next target, while HTTP 400 and 413 end a retry-until-success combo immediately.'),
      target: 'combos',
    },
    {
      id: 'keys',
      keywords: 'api key secret encryption lmdb storage security',
      title: tr('Are my provider API keys stored securely?'),
      body: tr('Provider keys stay in the local LMDB environment under ~/.exoroute, encrypted with your master key. They are never returned to the browser after creation and are masked in the dashboard.'),
      target: 'providers',
    },
    {
      id: 'quota',
      keywords: 'quota exhausted breaker circuit cooldown unhealthy',
      title: tr('What happens when a provider quota runs out?'),
      body: tr('A provider that keeps failing is marked unhealthy for a cooldown, combos fail over to healthy targets, and the next request after the cooldown probes that provider again.'),
      target: 'quota',
    },
    {
      id: 'protocol',
      keywords: 'protocol translate openai anthropic responses messages compatible',
      title: tr('Can I use a different protocol upstream?'),
      body: tr('Yes. ExoRoute translates between Chat Completions, Responses, and Anthropic Messages, so a client and its upstream provider can speak different protocols.'),
      target: 'providers',
    },
    {
      id: 'rate-limit',
      keywords: '429 rate limit retry-after throttle gateway key',
      title: tr('Why did a request return 429?'),
      body: tr('Either the gateway key spent its rate-limit budget or every combo target is rate-limited upstream. Honor the Retry-After header, then check the Requests page for the attempted targets.'),
      target: 'requests',
    },
    {
      id: 'backup',
      keywords: 'backup export import lmdb restore master key',
      title: tr('How do I back up my configuration?'),
      body: tr('Export an LMDB archive from Settings → Database backup. Keep EXOROUTE_MASTER_KEY separate: a restored archive needs it to decrypt provider secrets.'),
      target: 'settings',
    },
  ];
}

/** Status codes with the operator-facing cause of each one. */
export function errorCodes(tr: Translate): HelpErrorCode[] {
  const details = [
    tr('The ExoRoute gateway key is missing or invalid. Create one on the API keys page.'),
    tr('The key is valid, but its provider or model scope blocks this request.'),
    tr('No enabled combo or provider model matches the requested model name.'),
    tr('The key spent its rate limit, or every target is rate-limited upstream. Honor Retry-After.'),
    tr('Every target failed or rejected the request. The Requests page shows the upstream status per attempt.'),
    tr('The route has no enabled target, storage is busy, or the gateway is shutting down.'),
    tr('The request exceeded the total deadline across attempts and retries.'),
  ];
  return ERROR_REASONS.map((entry, index) => ({
    ...entry,
    detail: details[index],
  }));
}

/** Case-insensitive substring match over the caller-provided search text. */
export function filterHelpItems<T>(
  items: T[],
  query: string,
  toSearchText: (item: T) => string,
): T[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return items;
  return items.filter((item) => toSearchText(item).toLowerCase().includes(needle));
}
