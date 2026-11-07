/**
 * Live statistics refresh scheduling for the Requests page.
 *
 * The page already receives every finished request over the live SSE
 * stream, and a finished request is exactly when the server-side
 * aggregates change. Instead of recomputing totals on the client — which
 * drifts the moment an event is missed and cannot place a point into the
 * series buckets — each finish schedules one authoritative refetch: bursts
 * coalesce into a single fetch at most every `minIntervalMs`, a hidden tab
 * skips the fetch, and returning to the tab fetches one fresh snapshot so
 * events skipped while hidden cannot leave the summary stale.
 */

export interface StatisticsLiveRefresherOptions {
  /** Minimum spacing between live-triggered refetches. */
  minIntervalMs: number;
  /** Performs the (silent) statistics reload. */
  refresh: () => void;
  setTimeoutFn?: (callback: () => void, ms: number) => unknown;
  clearTimeoutFn?: (handle: unknown) => void;
  /** Returns whether the dashboard tab is currently hidden. */
  isHidden?: () => boolean;
  /** Subscribes to visibility changes; returns an unsubscribe function. */
  onVisibilityChange?: (handler: () => void) => () => void;
}

export interface StatisticsLiveRefresher {
  /** Coalesces a burst of finished requests into one scheduled refetch. */
  schedule: () => void;
  /** Cancels the pending timer and the visibility subscription. */
  dispose: () => void;
}

export function createStatisticsLiveRefresher(options: StatisticsLiveRefresherOptions): StatisticsLiveRefresher {
  const setTimeoutFn = options.setTimeoutFn ?? ((callback, ms) => window.setTimeout(callback, ms));
  const clearTimeoutFn = options.clearTimeoutFn ?? ((handle) => window.clearTimeout(handle as number));
  const isHidden = options.isHidden ?? (() => typeof document !== 'undefined' && document.visibilityState === 'hidden');
  const subscribeVisibility = options.onVisibilityChange ?? subscribeDocumentVisibility;

  let timer: unknown = null;
  let unsubscribe: (() => void) | null = null;

  function fire(): void {
    timer = null;
    if (isHidden()) return;
    options.refresh();
  }

  function schedule(): void {
    if (timer !== null) return;
    timer = setTimeoutFn(fire, options.minIntervalMs);
  }

  // Returning to the tab always fetches one fresh snapshot: events that
  // fired while hidden were dropped, not queued, so without this the
  // summary would stay stale until the next request finishes.
  unsubscribe = subscribeVisibility(() => {
    if (!isHidden()) schedule();
  });

  return {
    schedule,
    dispose: () => {
      if (timer !== null) {
        clearTimeoutFn(timer);
        timer = null;
      }
      unsubscribe?.();
      unsubscribe = null;
    },
  };
}

function subscribeDocumentVisibility(handler: () => void): () => void {
  if (typeof document === 'undefined') return () => {};
  document.addEventListener('visibilitychange', handler);
  return () => document.removeEventListener('visibilitychange', handler);
}
