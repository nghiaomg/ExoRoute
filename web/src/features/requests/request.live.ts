import { api } from '../../lib/api';
import type { RequestLiveConnectionState, RequestLiveEvent, RequestLog, RequestLogFilters } from '../../lib/types';
import { applyRequestLiveEvent, createRequestLiveState, type RequestLiveState } from './request.state';

type RequestLiveControllerOptions = {
  getState: () => RequestLiveState;
  getFilters: () => RequestLogFilters;
  onState: (state: RequestLiveState) => void;
  onFinished: (request: RequestLog) => void;
  onConnectionChange?: (state: RequestLiveConnectionState) => void;
  onClockChange?: (now: number) => void;
};

export type RequestLiveController = {
  start: () => void;
  stop: () => void;
};

export function createRequestLiveController(options: RequestLiveControllerOptions): RequestLiveController {
  let state = createRequestLiveState();
  let liveController: AbortController | null = null;
  let liveTimer: number | null = null;

  function syncLiveTimer(): void {
    if (state.requests.size > 0 && liveTimer === null) {
      liveTimer = window.setInterval(() => options.onClockChange?.(Date.now()), 1000);
    } else if (state.requests.size === 0 && liveTimer !== null) {
      window.clearInterval(liveTimer);
      liveTimer = null;
    }
  }

  function applyEvent(event: RequestLiveEvent): void {
    const result = applyRequestLiveEvent(options.getState(), event, options.getFilters());
    state = result.state;
    options.onState(state);
    if (result.finished) options.onFinished(result.finished);
    syncLiveTimer();
  }

  function clearUnavailableState(): void {
    state = { ...options.getState(), requests: new Map() };
    options.onState(state);
    syncLiveTimer();
  }

  function start(): void {
    if (liveController) return;
    const controller = new AbortController();
    liveController = controller;
    options.onConnectionChange?.('connecting');
    void api.streamRequestLive(applyEvent, controller.signal, (connectionState) => {
      if (!controller.signal.aborted) {
        options.onConnectionChange?.(connectionState);
        if (connectionState === 'unavailable') clearUnavailableState();
      }
    }).catch(() => {
      if (!controller.signal.aborted) {
        options.onConnectionChange?.('unavailable');
        clearUnavailableState();
      }
    });
  }

  function stop(): void {
    liveController?.abort();
    liveController = null;
    if (liveTimer !== null) window.clearInterval(liveTimer);
    liveTimer = null;
  }

  return { start, stop };
}
