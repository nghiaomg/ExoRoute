import { expect } from 'vitest';
import { screen, waitFor, fireEvent } from '@testing-library/svelte';
import type { WorkspaceChatModelsResult } from '../../../src/lib/types';
import type { Translate } from '../../../src/lib/format';

/** Identity translator mirroring the en catalog keys used in chat tests. */
export const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

export const modelsResult: WorkspaceChatModelsResult = {
  models: [{ provider_id: 'ocg', model: 'minimax-m3', id: 'ocg/minimax-m3' }],
  truncated: false,
};

/**
 * Waits for the model the page preselects and submits the message. The draft
 * always starts on a model, so a turn test never has to pick one first; the
 * picker displays the readable model name rather than the `{prefix}/{model}`
 * id the gateway receives.
 */
export async function sendWithDefaultModel(message: string): Promise<void> {
  const picker = (await screen.findByPlaceholderText('Select a model')) as HTMLInputElement;
  // Svelte writes the input's value property, never the attribute.
  await waitFor(() => expect(picker.value).toBe('minimax-m3'));

  const composer = screen.getByPlaceholderText('Type a message…');
  await fireEvent.input(composer, { target: { value: message } });
  await fireEvent.click(screen.getByTitle('Send'));
}
