import { render, screen, within } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import DocsLayout from '../../src/features/docs/DocsLayout.svelte';
import type { Locale } from '../../src/lib/i18n';
const tr = (key, vars) => key.replace(/\{(\w+)\}/g, (_, n) => String(vars?.[n] ?? `{${n}}`));
const preferences = { locale: 'en' as Locale, theme: 'light' as const, themePreference: 'light' as const, scheme: 'light' as const, setLocale: vi.fn(), setTheme: vi.fn(), setThemePreference: vi.fn() };
function renderLayout(page = 'docs-quickstart') {
  return render(DocsLayout, { props: { page, locale: 'en', preferences, onNavigate: vi.fn(), onExitDocs: vi.fn() } });
}
describe('standalone documentation workspace', () => {
  test('renders one documentation sidebar and a single language selector', () => {
    renderLayout();
    const sidebar = screen.getByRole('complementary', { name: /Documentation navigation/i });
    expect(sidebar).toBeTruthy();
    expect(screen.getAllByRole('combobox', { name: /Language/i })).toHaveLength(1);
    expect(screen.queryByPlaceholderText(/Tìm yêu cầu|Search requests/i)).toBeNull();
  });
  test('highlights the active docs route and offers prev/next navigation', () => {
    renderLayout('docs-quickstart');
    const sidebar = screen.getByRole('complementary', { name: /Documentation navigation/i });
    expect(within(sidebar).getByRole('link', { name: 'Quickstart' }).getAttribute('aria-current')).toBe('page');
    expect(screen.getByRole('navigation', { name: /On this page/i })).toBeTruthy();
  });
});
