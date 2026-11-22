import { ApiError } from './core';

export interface PingOptions {
  signal?: AbortSignal;
  timeoutMs?: number;
}

/**
 * Measures gateway responsiveness from a user-configured base URL. Unlike the
 * admin API helpers this hits the public /v1/models endpoint, but it still
 * provides a bounded timeout and honors an external AbortSignal so callers can
 * cancel an in-flight ping on re-trigger or unmount.
 */
export async function pingGateway(endpointBase: string, options: PingOptions = {}): Promise<number> {
  const timeoutMs = options.timeoutMs ?? 8000;
  const controller = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; controller.abort(); }, timeoutMs);
  const relayAbort = (): void => controller.abort();
  const external = options.signal;
  if (external) {
    if (external.aborted) relayAbort();
    else external.addEventListener('abort', relayAbort, { once: true });
  }
  const started = Date.now();
  try {
    const response = await fetch(`${endpointBase.replace(/\/$/, '')}/v1/models`, {
      method: 'GET',
      headers: { Accept: 'application/json' },
      signal: controller.signal,
    });
    if (!response.ok) throw new ApiError(`gateway returned HTTP ${response.status}`, response.status);
    return Date.now() - started;
  } catch (error) {
    if (timedOut) throw new ApiError('gateway ping timed out', 0);
    throw error;
  } finally {
    clearTimeout(timer);
    external?.removeEventListener('abort', relayAbort);
  }
}
