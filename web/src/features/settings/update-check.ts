import { onDestroy, onMount } from 'svelte';
import { api } from '../../lib/api';
import type { UpdateCheckResult } from '../../lib/types';

export type UpdateCheckStatus = {
  result: UpdateCheckResult | null;
  loading: boolean;
  failed: boolean;
};

export type UpdateCheckStore = {
  subscribe: (run: (status: UpdateCheckStatus) => void) => () => void;
  check: () => void;
};

/**
 * Owns the daily update-check lifecycle for the settings server tab:
 * interval scheduling, generation guards against late responses, and
 * teardown. The store value is the current status; the component stays
 * presentational.
 */
export function createUpdateCheckStore(): UpdateCheckStore {
  let status: UpdateCheckStatus = { result: null, loading: false, failed: false };
  const subscribers = new Set<(status: UpdateCheckStatus) => void>();
  let generation = 0;
  let timer: number | undefined;

  function emit(): void {
    for (const run of subscribers) run(status);
  }

  function set(next: Partial<UpdateCheckStatus>): void {
    status = { ...status, ...next };
    emit();
  }

  async function check(): Promise<void> {
    if (status.loading) return;
    const requestGeneration = ++generation;
    set({ loading: true, failed: false });
    try {
      const result = await api.updateCheck();
      if (requestGeneration !== generation) return;
      set({ result, loading: false });
    } catch {
      if (requestGeneration !== generation) return;
      set({ result: null, loading: false, failed: true });
    }
  }

  onMount(() => {
    void check();
    timer = window.setInterval(() => { void check(); }, 24 * 60 * 60 * 1000);
  });

  onDestroy(() => {
    generation += 1;
    if (timer !== undefined) window.clearInterval(timer);
  });

  return {
    subscribe: (run) => {
      subscribers.add(run);
      run(status);
      return () => {
        subscribers.delete(run);
      };
    },
    check: () => { void check(); },
  };
}
