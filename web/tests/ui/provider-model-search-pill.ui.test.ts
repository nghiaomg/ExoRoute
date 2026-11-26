import { render, screen } from '@testing-library/svelte';
import { describe, expect, test } from 'vitest';
import ProviderModelTests from '../../src/features/providers/ProviderModelTests.svelte';
import type { Translate } from '../../src/lib/format';
import type { Provider } from '../../src/lib/types';
const tr: Translate = (k) => k;
const provider: Provider = { id: 'p1', adapter_id: 'p1', name: 'P', base_url: 'http://localhost', enabled: true, auth_type: 'none', preferred_protocol: 'chat_completions', supported_protocols: ['chat_completions'] };
describe('saved-model search pill', () => {
  test('search input is wrapped in a focus delegation label and carries no border classes', () => {
    render(ProviderModelTests, { props: { provider, models: [{ provider_id: 'p1', model: 'm1' }], tr, onDeleteModel: async () => {} } });
    const input = screen.getByLabelText('Search saved models') as HTMLInputElement;
    expect(input.tagName).toBe('INPUT');
    const wrapper = input.closest('label.search-box');
    expect(wrapper).toBeTruthy();
    expect(wrapper?.querySelector('svg')).toBeTruthy();
    expect(input.className).not.toMatch(/border|ring|outline/);
  });
});
