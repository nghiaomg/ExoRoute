<script lang="ts">
  import Button from '../../components/ui/Button.svelte';
  import LanguageSelect from '../../components/layout/LanguageSelect.svelte';
  import { Copy, Menu, Moon, Sun, Terminal, X } from '@lucide/svelte';
  import { t, type Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { DocsPage, Page } from '../../lib/navigation';
  import { THEMES, type ThemeId, type ThemePreference } from '../../lib/theme';
  import { docsCopy } from './docsCopy';
  interface Preferences { locale: Locale; theme: ThemeId; themePreference: ThemePreference; scheme: 'light' | 'dark'; setLocale: (locale: Locale) => void; setTheme: (theme: ThemeId) => void; setThemePreference: (p: ThemePreference) => void }
  interface Props { page: DocsPage; locale: Locale; preferences: Preferences; providerCount?: number; gatewayAddress?: string; onNavigate: (page: Page) => void; onExitDocs: () => void; children?: import('svelte').Snippet }
  let { page, locale, preferences, providerCount = 0, gatewayAddress = '127.0.0.1:8686', onNavigate, onExitDocs, children }: Props = $props();
  let tr: Translate = $derived((key, vars) => t(locale, key, vars));
  let copy = $derived(docsCopy(locale));
  let menuOpen = $state(false);
  let query = $state('');
  const navigation: ReadonlyArray<DocsPage> = ['docs', 'docs-quickstart', 'docs-integrations', 'docs-reference'];
  const ordered: ReadonlyArray<DocsPage> = ['docs-quickstart', 'docs-integrations', 'docs-reference'];
  const visible = $derived(navigation.filter((item) => {
    const q = query.trim().toLowerCase();
    return !q || `${copy.nav[item].label} ${copy.nav[item].description}`.toLowerCase().includes(q);
  }));
  const activeNav = $derived(copy.nav[page]);
  const currentIndex = $derived(Math.max(0, ordered.indexOf(page)));
  const previous = $derived(currentIndex > 0 ? ordered[currentIndex - 1] : null);
  const next = $derived(currentIndex < ordered.length - 1 ? ordered[currentIndex + 1] : null);
  function pathFor(nextPage: DocsPage): string { return nextPage === 'docs' ? '/docs' : `/${nextPage.replace('docs-', 'docs/')}`; }
  function navigate(event: MouseEvent, target: DocsPage): void {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault(); menuOpen = false; onNavigate(target);
  }
  async function copyBaseUrl(): Promise<void> {
    const baseUrl = `http://${gatewayAddress}/v1`;
    try { await navigator.clipboard.writeText(baseUrl); showDashboardToast(`${copy.shell.copied}: ${baseUrl}`); }
    catch { showDashboardToast(copy.shell.copyFailed); }
  }
</script>

<svelte:head>
  <title>{activeNav.label} · ExoRoute</title>
  <meta name="description" content={activeNav.description} />
  <meta name="referrer" content="no-referrer" />
</svelte:head>

<div class="docs-workspace">
  {#if menuOpen}<button type="button" class="docs-scrim" aria-label={tr('Close documentation navigation')} onclick={() => (menuOpen = false)}></button>{/if}
  <aside class="docs-sidebar" class:open={menuOpen} aria-label={copy.shell.menu}>
    <div class="docs-sidebar-head">
      <a class="docs-brand" href="/docs" onclick={(e) => navigate(e, 'docs')}><span class="docs-brand-mark">ER</span><strong>ExoRoute</strong><span class="docs-version">0.1.3</span></a>
      <Button class="icon-button docs-close" type="button" aria-label={tr('Close documentation navigation')} onclick={() => (menuOpen = false)}><X size={16} /></Button>
      <Button class="action-button docs-back" type="button" onclick={() => { menuOpen = false; onExitDocs(); }}><span class="button-label">← {copy.shell.backToDashboard}</span></Button>
    </div>
    <label class="docs-search">
      <span class="sr-only">{tr('Search documentation…')}</span>
      <input type="search" bind:value={query} placeholder={tr('Search documentation…')} aria-label={tr('Search documentation…')} />
    </label>
    <nav class="docs-nav" aria-label={copy.shell.menu}>
      {#each visible as item (item)}
        <a href={pathFor(item)} class:active={page === item} aria-current={page === item ? 'page' : undefined} onclick={(event) => navigate(event, item)}>{copy.nav[item].label}</a>
      {:else}<p class="docs-nav-empty">{tr('No matching documentation sections')}</p>{/each}
    </nav>
    <div class="docs-sidebar-foot">
      <span class="docs-gateway-pill"><i></i>{tr('Gateway local-first')} · {gatewayAddress}</span>
      <label class="docs-theme-select"><span class="sr-only">{tr('All themes')}</span><select aria-label={tr('All themes')} value={preferences.themePreference} onchange={(e) => preferences.setThemePreference(e.currentTarget.value as ThemePreference)}><option value="system">{tr('System theme')}</option>{#each THEMES as theme}<option value={theme.id}>{theme.name}</option>{/each}</select></label>
    </div>
  </aside>

  <div class="docs-main">
    <header class="docs-topbar">
      <Button class="icon-button docs-menu" type="button" aria-label={tr('Open documentation navigation')} onclick={() => (menuOpen = true)}><Menu size={17} /></Button>
      <div class="docs-breadcrumb"><span>{copy.shell.documentation}</span><span class="crumb-sep">/</span><strong>{activeNav.label}</strong></div>
      <div class="docs-topbar-actions">
        <LanguageSelect locale={preferences.locale} {tr} onLocaleChange={preferences.setLocale} />
        <Button class="icon-button" type="button" aria-label={preferences.scheme === 'light' ? copy.shell.themeDark : copy.shell.themeLight} title={preferences.scheme === 'light' ? copy.shell.themeDark : copy.shell.themeLight} onclick={() => preferences.setTheme(preferences.scheme === 'light' ? 'dark' : 'light')}>
          {#if preferences.scheme === 'light'}<Moon size={15} />{:else}<Sun size={15} />{/if}
        </Button>
        <Button class="action-button" type="button" onclick={copyBaseUrl}><Copy size={14} /><span class="button-label">{tr('Copy base URL')}</span></Button>
        <Button class="action-button primary" type="button" onclick={() => onNavigate('chat')}><Terminal size={14} /><span class="button-label">{tr('Try in Chat')}</span></Button>
      </div>
    </header>
    <div class="docs-body">
      <main class="docs-article">
        {@render children?.()}
        {#if previous || next}
          <nav class="docs-pager" aria-label={tr('On this page')}>
            {#if previous}<a class="docs-pager-link prev" href={pathFor(previous)} onclick={(e) => navigate(e, previous)}><span>← {tr('Previous')}</span><strong>{copy.nav[previous].label}</strong></a>{:else}<span></span>{/if}
            {#if next}<a class="docs-pager-link next" href={pathFor(next)} onclick={(e) => navigate(e, next)}><span>{tr('Next')} →</span><strong>{copy.nav[next].label}</strong></a>{/if}
          </nav>
        {/if}
      </main>
      <aside class="docs-outline" aria-label={tr('On this page')}><h2>{tr('On this page')}</h2><a href={pathFor(page)} class:active={true}>{activeNav.label}</a></aside>
    </div>
  </div>
</div>

<style>
  .docs-workspace { display:flex; min-height:100dvh; color:var(--ink); background:var(--canvas); }
  .docs-sidebar { position:sticky; top:0; display:flex; flex-direction:column; width:260px; height:100dvh; flex:0 0 260px; gap:14px; padding:18px 14px; border-right:1px solid var(--line); background:var(--surface); z-index:30; }
  .docs-sidebar-head { display:grid; gap:10px; }
  .docs-brand { display:flex; align-items:center; gap:8px; text-decoration:none; color:var(--ink); }
  .docs-brand-mark { display:grid; place-items:center; width:28px; height:28px; border-radius:8px; background:var(--accent); color:var(--accent-contrast); font:700 11px var(--font-mono); }
  .docs-version { padding:2px 6px; border:1px solid var(--line); border-radius:6px; color:var(--muted); font-size:10px; }
  :global(.docs-close) { display:none; }
  :global(.docs-back) { width:100%; }
  .docs-search input { width:100%; padding:8px 10px; border:1px solid var(--line); border-radius:9px; background:var(--surface-sunken); color:var(--ink); font-size:12px; }
  .docs-nav { display:grid; gap:3px; }
  .docs-nav a { display:block; padding:9px 11px; border-radius:9px; color:var(--muted); font-size:13px; font-weight:500; text-decoration:none; }
  .docs-nav a:hover { background:var(--surface-sunken); color:var(--ink); }
  .docs-nav a.active { background:color-mix(in srgb, var(--accent) 12%, var(--surface)); color:var(--accent); font-weight:600; }
  .docs-nav-empty { color:var(--muted); font-size:11px; margin:0; }
  .docs-sidebar-foot { margin-top:auto; display:grid; gap:8px; padding-top:12px; border-top:1px solid var(--line); }
  .docs-gateway-pill { display:inline-flex; align-items:center; gap:6px; color:var(--muted); font-size:11px; }
  .docs-gateway-pill i { width:6px; height:6px; border-radius:50%; background:var(--chart-good); }
  .docs-theme-select select { width:100%; padding:6px 8px; border:1px solid var(--line); border-radius:8px; background:var(--surface-sunken); color:var(--ink); font-size:11px; }
  .docs-main { flex:1; min-width:0; }
  .docs-topbar { position:sticky; top:0; z-index:20; display:flex; align-items:center; gap:12px; min-height:56px; padding:8px 20px; border-bottom:1px solid var(--line); background:var(--surface); }
  :global(.docs-menu) { display:none; }
  .docs-breadcrumb { display:flex; align-items:center; gap:7px; min-width:0; font-size:12px; color:var(--muted); }
  .docs-breadcrumb strong { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--ink); }
  .crumb-sep { color:var(--muted); }
  .docs-topbar-actions { margin-left:auto; display:flex; align-items:center; gap:8px; flex-wrap:wrap; justify-content:flex-end; }
  .docs-body { display:grid; grid-template-columns:minmax(0,1fr) 200px; gap:24px; max-width:1400px; margin:0 auto; padding:28px 32px 60px; }
  .docs-article { min-width:0; }
  .docs-outline { display:grid; align-content:start; gap:5px; padding-left:14px; border-left:1px solid var(--line); }
  .docs-outline h2 { margin:0 0 6px; color:var(--muted); font-size:11px; text-transform:uppercase; letter-spacing:.08em; }
  .docs-outline a { color:var(--muted); font-size:12px; text-decoration:none; }
  .docs-outline a.active { color:var(--accent); font-weight:600; }
  .docs-pager { display:flex; justify-content:space-between; gap:12px; margin-top:32px; padding-top:18px; border-top:1px solid var(--line); }
  .docs-pager-link { display:grid; gap:3px; min-width:140px; padding:11px 14px; border:1px solid var(--line); border-radius:11px; background:var(--surface); color:var(--ink); text-decoration:none; }
  .docs-pager-link.next { text-align:right; }
  .docs-pager-link span { color:var(--muted); font-size:11px; }
  .docs-scrim { display:none; }
  @media (max-width:1279px){ .docs-outline { display:none; } .docs-body { grid-template-columns:minmax(0,1fr); } }
  @media (max-width:1023px){
    .docs-sidebar { position:fixed; inset:0 auto 0 0; transform:translateX(-100%); transition:transform .2s ease; box-shadow:none; }
    .docs-sidebar.open { transform:translateX(0); }
    :global(.docs-close), :global(.docs-menu) { display:grid; }
    .docs-scrim { display:block; position:fixed; inset:0; z-index:25; border:0; background:color-mix(in srgb, var(--ink) 35%, transparent); }
    .docs-body { padding:20px 16px 48px; }
    .docs-topbar { padding:8px 12px; }
  }
  @media (max-width:640px){ .docs-topbar-actions :global(.button-label) { display:none; } .docs-topbar-actions { gap:5px; } }
</style>
