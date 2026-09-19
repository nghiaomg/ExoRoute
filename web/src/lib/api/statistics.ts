import type { RequestStatistics } from '../types';
import { request } from './core';

export const statisticsApi = {
  statistics: (range: RequestStatistics['range'], signal?: AbortSignal) =>
    request<RequestStatistics>(`/statistics?range=${range}`, signal ? { signal } : {}),
};
