import type { WorkspaceChatInput, WorkspaceChatModelsResult, WorkspaceChatResult } from '../types';
import { request } from './core';

/**
 * Workspace chat endpoints. The relay deliberately persists nothing, so this
 * client keeps every conversation turn in memory only.
 */
export const workspaceChatApi = {
  chat: (input: WorkspaceChatInput, signal?: AbortSignal) =>
    request<WorkspaceChatResult>('/workspace/chat', {
      method: 'POST',
      body: JSON.stringify(input),
      ...(signal ? { signal } : {}),
    }),
  chatModels: (signal?: AbortSignal) =>
    request<WorkspaceChatModelsResult>('/workspace/chat/models', signal ? { signal } : {}),
};
