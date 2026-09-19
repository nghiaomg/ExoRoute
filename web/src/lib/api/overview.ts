import type { Overview, RequestLiveConnectionState, RequestLiveEvent, UpstreamLiveSnapshot } from '../types';
import { adminSseDependencies, request } from './core';
import { streamRequestLiveEvents, streamUpstreamLiveEvents } from '../sseClient';

export const overviewApi = {
overview: () => request<Overview>('/overview'),
streamUpstreamLive: (onSnapshot: (snapshot: UpstreamLiveSnapshot) => void, signal: AbortSignal) => streamUpstreamLiveEvents(onSnapshot, signal, adminSseDependencies),
streamRequestLive: (
  onEvent: (event: RequestLiveEvent) => void,
  signal: AbortSignal,
  onStatus?: (state: RequestLiveConnectionState) => void,
) => streamRequestLiveEvents(onEvent, signal, adminSseDependencies, onStatus),
};



