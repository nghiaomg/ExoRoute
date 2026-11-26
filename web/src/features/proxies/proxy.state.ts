import type { ProxyEndpoint, GatewayProxy } from '../../lib/types';

export function parseBulkProxyEndpoints(text: string, tr: (key: string, vars?: Record<string, string | number>) => string): { endpoints: ProxyEndpoint[]; errors: string[] } {
  const endpoints: ProxyEndpoint[] = [];
  const errors: string[] = [];
  const lines = text.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const raw = lines[index].trim();
    if (!raw || raw.startsWith('#')) continue;
    const [address, label] = raw.split(/\s+#\s*/, 2);
    const url = address.trim();
    if (!url || url.length > 512) {
      errors.push(tr('Line {line}: proxy address is empty or longer than 512 characters.', { line: index + 1 }));
      continue;
    }
    endpoints.push({
      endpoint_id: crypto.randomUUID(),
      url,
      priority: endpoints.length,
      enabled: true,
      ...(label ? { label: label.trim().slice(0, 64) } : {}),
    });
  }
  if (endpoints.length === 0 && errors.length === 0) errors.push(tr('Paste at least one proxy address.'));
  if (endpoints.length > 200) errors.push(tr('A proxy import may contain at most 200 lines.'));
  return { endpoints, errors };
}

export function blankProxy(name = ''): Omit<GatewayProxy, 'id' | 'created_at' | 'updated_at' | 'provider_count'> {
  return {
    name,
    strategy: 'priority',
    enabled: true,
    endpoints: [{ endpoint_id: crypto.randomUUID(), url: '', priority: 0, enabled: true }],
  };
}

export function normalizeProxyDraft(endpoint: ProxyEndpoint): ProxyEndpoint {
  const normalized: ProxyEndpoint = {
    ...endpoint,
    url: endpoint.url?.trim() || null,
    ref_proxy_id: endpoint.ref_proxy_id?.trim() || null,
    username: endpoint.username?.trim() || null,
    secret: endpoint.secret || null,
    label: endpoint.label?.trim() || null,
  };
  if (normalized.ref_proxy_id) {
    normalized.url = null;
    normalized.username = null;
    normalized.secret = null;
  }
  return normalized;
}

export function endpointDisplay(endpoint: ProxyEndpoint): string {
  if (endpoint.ref_proxy_id) return `Proxy group: ${endpoint.ref_proxy_id}`;
  return endpoint.url ?? '';
}
