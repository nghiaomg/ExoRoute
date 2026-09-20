<script lang="ts">
  import { BookOpen, Braces, ChevronRight, Code2, Menu, Moon, Network, Sun, X } from '@lucide/svelte';
  import FlyingFishLogo from '../../components/FlyingFishLogo.svelte';
  import LanguageSelect from '../../components/LanguageSelect.svelte';
  import { t, type Locale } from '../../lib/i18n';
  import type { DocsPage } from '../../lib/navigation';
  import { docsCopy } from './docsCopy';

  type Theme = 'light' | 'dark';
  interface Preferences {
    locale: Locale;
    theme: Theme;
    setLocale: (locale: Locale) => void;
    toggleTheme: () => void;
  }

  export let page: DocsPage;
  export let locale: Locale;
  export let preferences: Preferences;
  export let onNavigate: (page: DocsPage) => void;
  export let onBackToLogin: () => void;

  let menuOpen = false;
  $: copy = docsCopy(locale);
  $: activeNav = copy.nav[page];

  const navigation = [
    { page: 'docs', icon: BookOpen },
    { page: 'docs-quickstart', icon: Code2 },
    { page: 'docs-integrations', icon: Network },
    { page: 'docs-reference', icon: Braces },
  ] as const;

  function navigate(event: MouseEvent, nextPage: DocsPage): void {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    menuOpen = false;
    onNavigate(nextPage);
  }

  function pathFor(nextPage: DocsPage): string {
    return nextPage === 'docs' ? '/docs' : `/${nextPage.replace('docs-', 'docs/')}`;
  }

  function backToDashboard(event: MouseEvent): void {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    onBackToLogin();
  }
</script>

<svelte:head>
  <title>{activeNav.label} · ExoRoute</title>
  <meta name="description" content={activeNav.description} />
  <meta name="referrer" content="no-referrer" />
</svelte:head>

<div class="docs-shell">
  <header class="docs-topbar">
    <a class="docs-brand" href="/docs" onclick={(event) => navigate(event, 'docs')} aria-label="ExoRoute documentation">
      <span class="docs-brand-mark"><FlyingFishLogo size={24} variant="mark" /></span>
      <span class="brand-word">exo<span>route</span></span>
      <span class="docs-brand-divider"></span>
      <span class="docs-brand-section">{copy.shell.documentation}</span>
    </a>

    <div class="docs-topbar-actions">
      <span class="docs-local-badge"><span class="docs-status-dot"></span>{copy.shell.localFirst}</span>
      <div class="preference-controls" aria-label="Language and appearance">
        <LanguageSelect locale={preferences.locale} tr={(key) => t(locale, key)} onLocaleChange={preferences.setLocale} />
        <button class="preference-button theme-button" aria-label={preferences.theme === 'light' ? copy.shell.themeDark : copy.shell.themeLight} title={preferences.theme === 'light' ? copy.shell.themeDark : copy.shell.themeLight} aria-pressed={preferences.theme === 'dark'} onclick={preferences.toggleTheme}>
          {#if preferences.theme === 'light'}<Moon size={15} />{:else}<Sun size={15} />{/if}
        </button>
      </div>
      <a class="docs-sign-in" href="/login" onclick={backToDashboard}>{copy.shell.signIn}<ChevronRight size={15} /></a>
      <button class="docs-menu-button" type="button" aria-label={copy.shell.menu} aria-expanded={menuOpen} onclick={() => menuOpen = true}><Menu size={20} /></button>
    </div>
  </header>

  <div class="docs-body">
    <aside class="docs-sidebar" aria-label={copy.shell.menu}>
      <div class="docs-sidebar-label">{copy.shell.publicDocs}</div>
      <nav class="docs-nav">
        {#each navigation as item}
          <a class:active={page === item.page} class="docs-nav-item" href={pathFor(item.page)} onclick={(event) => navigate(event, item.page)}>
            <svelte:component this={item.icon} size={17} strokeWidth={1.8} />
            <span><strong>{copy.nav[item.page].label}</strong><small>{copy.nav[item.page].description}</small></span>
          </a>
        {/each}
      </nav>
      <div class="docs-sidebar-note">
        <span class="docs-note-icon"><BookOpen size={15} /></span>
        <div><strong>{copy.shell.localFirst}</strong><small>{copy.home.securityBody}</small></div>
      </div>
    </aside>

    <main class="docs-main">
      <div class="docs-mobile-heading">
        <div><span class="docs-kicker">{copy.shell.publicDocs}</span><strong>{activeNav.label}</strong></div>
        <button class="docs-mobile-menu-toggle" type="button" aria-label={copy.shell.menu} aria-expanded={menuOpen} onclick={() => menuOpen = true}><Menu size={18} /></button>
      </div>
      <div class="docs-content"><slot /></div>
      <footer class="docs-footer">
        <span><FlyingFishLogo size={14} variant="monochrome" /> ExoRoute · {copy.shell.localFirst}</span>
        <a href="/login" onclick={backToDashboard}>{copy.shell.backToDashboard} <ChevronRight size={14} /></a>
      </footer>
    </main>
  </div>

  {#if menuOpen}
    <div class="docs-mobile-backdrop" role="presentation" onclick={() => menuOpen = false}></div>
    <div class="docs-mobile-sheet" role="dialog" aria-modal="true" aria-label={copy.shell.menu}>
      <div class="docs-mobile-sheet-header">
        <div><span class="docs-kicker">{copy.shell.publicDocs}</span><strong>{copy.shell.documentation}</strong></div>
        <button class="icon-button" type="button" aria-label={copy.shell.closeMenu} onclick={() => menuOpen = false}><X size={19} /></button>
      </div>
      <nav class="docs-mobile-nav">
        {#each navigation as item}
          <a class:active={page === item.page} href={pathFor(item.page)} onclick={(event) => navigate(event, item.page)}>
            <svelte:component this={item.icon} size={18} strokeWidth={1.8} />
            <span><strong>{copy.nav[item.page].label}</strong><small>{copy.nav[item.page].description}</small></span>
            <ChevronRight size={16} />
          </a>
        {/each}
      </nav>
      <a class="docs-mobile-login" href="/login" onclick={backToDashboard}>{copy.shell.signIn}<ChevronRight size={15} /></a>
    </div>
  {/if}
</div>
