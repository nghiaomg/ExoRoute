//! Secret-redacted diagnostic report builder. Only explicitly allow-listed
//! non-sensitive fields are ever included: no API keys, tokens, cookies,
//! headers, user prompts, or raw provider payloads.

import type { Overview, RequestStatistics, PublicSettings } from '../../lib/types';
import { DASHBOARD_VERSION } from '../../lib/version';

export interface DiagnosticInput {
  overview: Overview | null;
  stats: RequestStatistics | null;
  settings: PublicSettings;
  endpoint: string;
  gatewayStatus: string;
  retention: number | null;
  maxRows: number | null;
}

/** Removes anything that looks like a credential before it can reach the report. */
export function redact(value: string): string {
  return value
    .replace(/sk-[A-Za-z0-9_-]{8,}/gi, '<redacted>')
    .replace(/\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b/g, '<redacted>')
    .replace(/(Authorization|x-api-key|api[_-]?key|token|secret|password)=[^\s,&]+/gi, '$1=<redacted>')
    .replace(/Bearer\s+[A-Za-z0-9._~+/=-]{6,}/gi, 'Bearer <redacted>');
}

export function buildDiagnosticReport(input: DiagnosticInput, tr: (key: string, vars?: Record<string, string | number>) => string): string {
  const report: string[] = [
    `# ExoRoute ${tr('Diagnostic report')}`
    , `- ${tr('Version')}: ${DASHBOARD_VERSION}`
    , `- ${tr('Gateway endpoint')}: ${redact(input.endpoint)}`
    , `- ${tr('Gateway status')}: ${input.gatewayStatus}`
    , `- ${tr('Providers')}: ${input.overview?.provider_count ?? '?'} · ${tr('Combos')}: ${input.overview?.combo_count ?? '?'}`
    , `- ${tr('Requests')}: ${input.overview?.request_count ?? '?'}`
    , `- ${tr('24h success rate')}: ${input.stats ? input.stats.success_rate.toFixed(1) + '%' : '?'}`
    , `- ${tr('24h average latency')}: ${input.stats ? Math.round(input.stats.average_latency_ms) + ' ms' : '?'}`
    , `- ${tr('Storage engine')}: LMDB`
    , `- ${tr('Request log retention')}: ${input.retention ?? '?'} ${tr('days')} · ${input.maxRows ?? '?'} ${tr('records retained')}`
    , `> ${tr('No secrets are included in this report.')}`
  ];
  return report.map(redact).join('\n');
}
