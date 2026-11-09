/* ------------------------------------------------------------------ */
/* Deadline-bounded model list loading                                 */
/* ------------------------------------------------------------------ */

/** How long the picker waits for the model list before it reports a timeout. */
export const MODEL_LIST_TIMEOUT_MS = 20_000;

/** Exactly one outcome applies to a deadline-bounded request. */
export type DeadlineResult<T> =
  | { kind: 'ok'; value: T }
  | { kind: 'error'; error: unknown }
  | { kind: 'timeout' }
  | { kind: 'cancelled' };

export interface DeadlineRace<T> {
  outcome: Promise<DeadlineResult<T>>;
  cancel: () => void;
}

/**
 * Races one request against a deadline this page owns. An abort alone is not
 * enough: the shared admin client can queue behind a cross-tab session refresh
 * that never observes the abort signal, so the request promise may stay
 * pending long after its controller fired. The deadline guarantees an outcome
 * the UI can render on its own, and `cancel` releases the request when the page
 * unmounts or a newer load supersedes it.
 */
export function raceWithDeadline<T>(
  request: (signal: AbortSignal) => Promise<T>,
  timeoutMs: number,
): DeadlineRace<T> {
  const controller = new AbortController();
  let settle: (result: DeadlineResult<T>) => void = () => {};
  const outcome = new Promise<DeadlineResult<T>>((resolve) => {
    settle = resolve;
  });
  let timer: ReturnType<typeof setTimeout> | null = setTimeout(() => {
    timer = null;
    controller.abort();
    settle({ kind: 'timeout' });
  }, timeoutMs);
  const clear = (): void => {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  };
  void (async () => {
    try {
      const value = await request(controller.signal);
      clear();
      settle({ kind: 'ok', value });
    } catch (error) {
      clear();
      settle({ kind: 'error', error });
    }
  })();
  return {
    outcome,
    cancel: () => {
      clear();
      controller.abort();
      settle({ kind: 'cancelled' });
    },
  };
}
