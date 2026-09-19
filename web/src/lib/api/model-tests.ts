import type { ModelTestResponse, ModelTestTarget } from '../types';
import { request } from './core';

export const modelTestApi = {
  testModels: (models: ModelTestTarget[]) =>
    request<ModelTestResponse>('/models/test', {
      method: 'POST',
      body: JSON.stringify({ models }),
    }),
};
