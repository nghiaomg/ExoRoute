<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { Copy, Moon, Sun, Terminal } from '@lucide/svelte';
  import type { Locale } from '../../lib/i18n';
  import { t } from '../../lib/i18n';
  import type { DocsPage, Page } from '../../lib/navigation';
  import LanguageSelect from '../../components/LanguageSelect.svelte';
  import DashboardAppShell from '../../components/dashboard/DashboardAppShell.svelte';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import { docsCopy } from './docsCopy';
  import type { ThemeId, ThemeScheme } from '../../lib/theme';

  interface Preferences {
    locale: Locale;
    theme: ThemeId;
    scheme: ThemeScheme;
    setLocale: (locale: Locale) => void;
    toggleTheme: () => void;
  }

  export let page: DocsPage;
  export let locale: Locale;
  export let preferences: Preferences;
  export let onNavigate: (page: Page) => void;
  export let onExitDocs: () => void;
  export let onSignOut: (() => void) | undefined = undefined;
  export let providerCount = 0;
  export let gatewayAddress = '127.0.0.1:8686';

  $: copy = docsCopy(locale);
  $: activeNav = copy.nav[page];
  $: heading = page === 'docs' ? t(locale, 'Docs') : activeNav.label;

  const navigation = [
    { page: 'docs' },
    { page: 'docs-quickstart' },
    { page: 'docs-integrations' },
    { page: 'docs-reference' },
  ] as const;

  function navigate(event: MouseEvent, nextPage: DocsPage): void {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    onNavigate(nextPage);
  }

  function navigateFromShell(target: string): void {
    onNavigate(target as Page);
  }

  function pathFor(nextPage: DocsPage): string {
    return nextPage === 'docs' ? '/docs' : `/${nextPage.replace('docs-', 'docs/')}`;
  }

  async function copyBaseUrl(): Promise<void> {
    const baseUrl = `${window.location.origin}/v1`;
    try {
      await navigator.clipboard.writeText(baseUrl);
      showDashboardToast(`${copy.shell.copied}: ${baseUrl}`);
    } catch {
      showDashboardToast(copy.shell.copyFailed);
    }
  }
</script>

<svelte:head>
  <title>{activeNav.label} · ExoRoute</title>
  <meta name="description" content={activeNav.description} />
  <meta name="referrer" content="no-referrer" />
</svelte:head>

<DashboardAppShell page="docs" activePage="docs" {providerCount} {gatewayAddress} locale={preferences.locale} tr={(key) => t(locale, key)} onLocaleChange={preferences.setLocale} onNavigate={navigateFromShell} {onSignOut}>
  <div class="page-heading docs-route-heading max-nav:flex-col! max-nav:items-stretch!">
    <div class="heading-copy">
      <h1>{heading}</h1>
      <p>{activeNav.description}</p>
    </div>
    <div class="heading-actions docs-route-actions max-nav:flex-wrap! max-nav:justify-start!">
      <LanguageSelect locale={preferences.locale} tr={(key) => t(locale, key)} onLocaleChange={preferences.setLocale} />
      <Button
        class="icon-button"
        type="button"
        aria-label={preferences.scheme === 'light' ? copy.shell.themeDark : copy.shell.themeLight}
        title={preferences.scheme === 'light' ? copy.shell.themeDark : copy.shell.themeLight}
        aria-pressed={preferences.scheme === 'dark'}
        onclick={preferences.toggleTheme}
      >
        {#if preferences.scheme === 'light'}<Moon size={15} />{:else}<Sun size={15} />{/if}
      </Button>
      <Button class="action-button" type="button" onclick={copyBaseUrl}>
        <span class="button-icon"><Copy size={15} /></span>
        <span class="button-label">{t(locale, 'Copy base URL')}</span>
      </Button>
      <Button class="action-button primary" type="button" onclick={() => navigateFromShell('chat')}>
        <span class="button-icon"><Terminal size={15} /></span>
        <span class="button-label">{t(locale, 'Try in Chat')}</span>
      </Button>
    </div>
  </div>

  <div class="grid grid-cols-[230px_minmax(0,1fr)] gap-[18px] items-start max-[1100px]:grid-cols-1">
    <aside class="course-card surface sticky top-[18px] px-4! py-5! flex flex-col gap-3 max-[1100px]:static" aria-label={copy.shell.menu}>
      <p class="eyebrow">{copy.shell.documentation}</p>
      <nav class="flex flex-col gap-0.5" aria-label={copy.shell.menu}>
        {#each navigation as item (item.page)}
          <a class="block p-3 rounded-[10px] text-[13px] font-medium text-muted no-underline transition-[background,color] duration-150 hover:bg-surface-sunken hover:text-ink {page === item.page ? 'bg-db-accent-tint text-accent-strong font-semibold' : ''}" href={pathFor(item.page)} onclick={(event) => navigate(event, item.page)}>
            {copy.nav[item.page].label}
          </a>
        {/each}
      </nav>
      <div class="mt-auto flex items-center justify-between pt-2.5 border-t border-line [&_.status]:text-[11px]">
        <span class="status complete">{copy.shell.localFirst}</span>
        <Button class="mini-add" type="button" onclick={onExitDocs}>
          <span>{onSignOut ? copy.shell.backToDashboard : copy.shell.signIn}</span>
          <svg class="icon"><use href="#i-arrow-right" /></svg>
        </Button>
      </div>
    </aside>

    <div class="docs-content w-auto! m-0! p-0! flex min-w-0 flex-col gap-[18px]">
      <div class="docs-shell docs-embedded-content w-auto! h-auto! min-h-0! overflow-visible! bg-transparent!">
        <slot />
      </div>
    </div>
  </div>
</DashboardAppShell>
