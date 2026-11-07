/**
 * Live statistics refresh scheduling (request.stats.refresh.ts) and its
 * wiring into the Requests page.
 *
 * The scheduler must coalesce a burst of finished requests into a single
 * refetch, skip hidden tabs, refresh once when the tab becomes visible
 * again, and release everything on dispose. The source assertions pin the
 * page wiring: finished live events schedule it and unmount disposes it —
 * the integration whose regression would silently disable realtime stats.
 */
import { strict as assert } from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { createStatisticsLiveRefresher } from '../src/features/requests/request.stats.refresh';

type Timer = { callback: () => void; ms: number; cancelled: boolean };

/** Manual timer queue: flush() runs every non-cancelled callback in order. */
function makeClock() {
  const timers: Timer[] = [];
  return {
    timers,
    setTimeoutFn(callback: () => void, ms: number): unknown {
      const timer: Timer = { callback, ms, cancelled: false };
      timers.push(timer);
      return timer;
    },
    clearTimeoutFn(handle: unknown): void {
      (handle as Timer).cancelled = true;
    },
    flush(): void {
      for (const timer of timers.splice(0)) {
        if (!timer.cancelled) timer.callback();
      }
    },
    pendingCount(): number {
      return timers.filter((timer) => !timer.cancelled).length;
    },
  };
}

function makeVisibility(initiallyHidden: boolean) {
  let hidden = initiallyHidden;
  let handler: (() => void) | null = null;
  return {
    isHidden: () => hidden,
    onVisibilityChange: (next: () => void) => {
      handler = next;
      return () => { handler = null; };
    },
    setVisible(nowHidden: boolean): void {
      hidden = nowHidden;
      handler?.();
    },
    unsubscribed: () => handler === null,
  };
}

test('a burst of finished requests coalesces into one refetch', () => {
  const clock = makeClock();
  let refetches = 0;
  const refresher = createStatisticsLiveRefresher({
    minIntervalMs: 3000,
    refresh: () => { refetches += 1; },
    setTimeoutFn: clock.setTimeoutFn,
    clearTimeoutFn: clock.clearTimeoutFn,
    isHidden: () => false,
    onVisibilityChange: () => () => {},
  });

  refresher.schedule();
  refresher.schedule();
  refresher.schedule();
  assert.equal(clock.pendingCount(), 1, 'only one timer survives a burst');

  clock.flush();
  assert.equal(refetches, 1);
  refresher.dispose();
});

test('a scheduled refetch waits at least the configured interval', () => {
  const clock = makeClock();
  const refresher = createStatisticsLiveRefresher({
    minIntervalMs: 3000,
    refresh: () => {},
    setTimeoutFn: clock.setTimeoutFn,
    clearTimeoutFn: clock.clearTimeoutFn,
    isHidden: () => false,
    onVisibilityChange: () => () => {},
  });

  refresher.schedule();
  clock.flush();
  refresher.schedule();
  assert.ok(clock.timers.every((timer) => timer.ms === 3000), 'every timer uses the minimum interval');
  refresher.dispose();
});

test('a hidden tab skips the fetch; becoming visible fetches one fresh snapshot', () => {
  const clock = makeClock();
  const visibility = makeVisibility(true);
  let refetches = 0;
  const refresher = createStatisticsLiveRefresher({
    minIntervalMs: 3000,
    refresh: () => { refetches += 1; },
    setTimeoutFn: clock.setTimeoutFn,
    clearTimeoutFn: clock.clearTimeoutFn,
    isHidden: visibility.isHidden,
    onVisibilityChange: visibility.onVisibilityChange,
  });

  refresher.schedule();
  clock.flush();
  assert.equal(refetches, 0, 'a hidden tab never fetches');

  visibility.setVisible(false);
  assert.equal(clock.pendingCount(), 1, 'returning to the tab schedules a refetch');
  clock.flush();
  assert.equal(refetches, 1);
  refresher.dispose();
});

test('dispose cancels the pending timer and the visibility subscription', () => {
  const clock = makeClock();
  const visibility = makeVisibility(false);
  let refetches = 0;
  const refresher = createStatisticsLiveRefresher({
    minIntervalMs: 3000,
    refresh: () => { refetches += 1; },
    setTimeoutFn: clock.setTimeoutFn,
    clearTimeoutFn: clock.clearTimeoutFn,
    isHidden: visibility.isHidden,
    onVisibilityChange: visibility.onVisibilityChange,
  });

  refresher.schedule();
  refresher.dispose();

  assert.ok(visibility.unsubscribed(), 'the visibility listener is released');
  clock.flush();
  assert.equal(refetches, 0, 'a disposed refresher never fetches');

  visibility.setVisible(false);
  assert.equal(clock.pendingCount(), 0, 'visibility changes after dispose schedule nothing');
});

test('the page schedules on finished live events and disposes on unmount', () => {
  const source = readFileSync(new URL('../src/features/requests/RequestsPage.svelte', import.meta.url), 'utf8');

  const onFinished = source.match(/onFinished: \(finished\) => \{[\s\S]*?\n    \},/);
  assert.ok(onFinished, 'the live controller declares an onFinished handler');
  assert.match(onFinished[0], /statisticsRefresher\.schedule\(\)/, 'a finished request schedules the statistics refetch');

  const onDestroy = source.match(/onDestroy\(\(\) => \{[\s\S]*?\n  \}\);/);
  assert.ok(onDestroy, 'the page registers onDestroy');
  assert.match(onDestroy[0], /statisticsRefresher\.dispose\(\)/, 'unmount disposes the refresher');
});
