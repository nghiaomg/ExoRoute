import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import ProxyEndpointEditor from '../../src/features/proxies/ProxyEndpointEditor.svelte';
import type { Translate } from '../../src/lib/format';
import type { ProxyEndpoint } from '../../src/lib/types';
const tr: Translate = (k) => k;
const endpoint: ProxyEndpoint = { endpoint_id: 'e1', url: 'http://10.0.0.1:8080', priority: 0, enabled: true };
describe('proxy endpoint mode toggle', () => {
  test('active segment exposes aria-pressed and data-active, click switches mode', async () => {
    const onChange = vi.fn();
    render(ProxyEndpointEditor, { props: { endpoint, index: 0, options: [], tr, optionsLoading: false, optionsQuery: '', hasMoreOptions: false, onOptionsSearch: () => {}, onOptionsMore: () => {}, onChange, onMove: () => {}, onRemove: () => {} } });
    const url = screen.getByRole('button', { name: 'Proxy URL' });
    const embed = screen.getByRole('button', { name: 'Embed proxy group' });
    expect(url.getAttribute('data-active')).toBe('true');
    expect(url.getAttribute('aria-pressed')).toBe('true');
    expect(embed.getAttribute('data-active')).toBe('false');
    expect(embed.getAttribute('aria-pressed')).toBe('false');
    await fireEvent.click(embed);
    expect(onChange).toHaveBeenCalled();
  });
});
