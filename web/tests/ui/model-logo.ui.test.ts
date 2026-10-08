import { render } from '@testing-library/svelte';
import { describe, expect, test } from 'vitest';
import claudeLogo from '../../../assets/models/claude.svg?url';
import openAiLogo from '../../../assets/models/openai.svg?url';
import RequestTableRow from '../../src/features/requests/RequestTableRow.svelte';
import type { Translate } from '../../src/lib/format';
import type { RequestLog } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

function renderModel(model: string): HTMLElement {
  const request: RequestLog = {
    id: 'log-1',
    model,
    created_at: '2026-10-05T10:00:00.000Z',
    status: 200,
    duration_ms: 100,
    input_tokens: 10,
    output_tokens: 5,
  };
  const { container } = render(RequestTableRow, {
    props: { tr, locale: 'en', request, liveClockMs: Date.now() },
  });
  const cell = container.querySelector('td.req-model');
  if (!cell) throw new Error('model cell not rendered');
  return cell as HTMLElement;
}

function logoOf(model: string): string | null {
  return renderModel(model).querySelector('img')?.getAttribute('src') ?? null;
}

describe('request model logo', () => {
  test('draws the vendor file for a recognised model and drops the monogram', () => {
    const cell = renderModel('ocg/minimax-m3');
    expect(cell.querySelector('img')?.getAttribute('src')).toBeTruthy();
    expect(cell.querySelector('.req-model-monogram')).toBeNull();
    // The id stays visible: the logo is decorative.
    expect(cell.textContent).toContain('ocg/minimax-m3');
  });

  test('resolves the vendor named by the model id, not just any asset', () => {
    expect(logoOf('openai/gpt-5')).toBe(openAiLogo);
    expect(logoOf('gpt-5.4')).toBe(openAiLogo);
    expect(logoOf('anthropic/claude-opus-4-6')).toBe(claudeLogo);
    expect(logoOf('glm-5.3')).not.toBe(openAiLogo);
  });

  test('falls back to the monogram when no vendor logo matches', () => {
    const cell = renderModel('live/preview-model');
    expect(cell.querySelector('img')).toBeNull();
    expect(cell.querySelector('.req-model-monogram')?.textContent).toBe('PR');
  });
});
