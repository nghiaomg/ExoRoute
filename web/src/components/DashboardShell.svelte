<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    Activity, Boxes, ChartPie, Ellipsis, Gauge, KeyRound, Layers3, LogOut, Menu, Moon, RefreshCw, Settings2, Sun, X,
  } from '@lucide/svelte';
  import FlyingFishLogo from './FlyingFishLogo.svelte';
  import LanguageSelect from './LanguageSelect.svelte';
  import type { DashboardPage } from '../lib/navigation';
  import type { Locale } from '../lib/i18n';
  import type { Translate } from '../lib/format';

  type Theme = 'light' | 'dark';
  type ConnectionState = 'idle' | 'loading' | 'loaded' | 'error';
  interface GatewayStatus { state: ConnectionState; address: string }
  interface Preferences {
    locale: Locale;
    theme: Theme;
    setLocale: (locale: Locale) => void;
    toggleTheme: () => void;
  }

  export let currentPage: DashboardPage;
  export let title: string;
  export let tr: Translate;
  export let preferences: Preferences;
  export let gateway: GatewayStatus;
  export let providerCount = 0;
  export let onNavigate: (page: DashboardPage) => void;
  export let onRefresh: () => void;
  export let onSignOut: () => Promise<void>;

  let signingOut = false;
  let signOutError = '';
  let mobileMoreOpen = false;

  const navigation = [
    { id: 'overview', label: 'Overview', icon: Gauge },
    { id: 'providers', label: 'Providers', icon: Boxes },
    { id: 'combos', label: 'Combos', icon: Layers3 },
    { id: 'quota', label: 'Quota', icon: Gauge },
    { id: 'api-keys', label: 'API keys', icon: KeyRound },
    { id: 'requests', label: 'Requests', icon: Activity },
    { id: 'statistics', label: 'Statistics', icon: ChartPie },
    { id: 'settings', label: 'Settings', icon: Settings2 },
  ] as const;

  const mobileNavItems = [
    { id: 'overview', label: 'Overview', icon: Gauge },
    { id: 'providers', label: 'Providers', icon: Boxes },
    { id: 'combos', label: 'Combos', icon: Layers3 },
    { id: 'requests', label: 'Requests', icon: Activity },
    { id: 'more', label: 'More', icon: Ellipsis },
  ] as const;

  $: isMoreActive = ['api-keys', 'quota', 'statistics', 'settings'].includes(currentPage);

  function navigateFromLink(event: MouseEvent, page: DashboardPage): void {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    mobileMoreOpen = false;
    onNavigate(page);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' && mobileMoreOpen) {
      mobileMoreOpen = false;
    }
  }

  async function signOut(): Promise<void> {
    if (signingOut) return;
    signingOut = true;
    signOutError = '';
    try {
      await onSignOut();
    } catch (error) {
      signOutError = error instanceof Error ? error.message : tr('Could not sign out. Try again.');
    } finally {
      signingOut = false;
    }
  }

  onMount(() => {
    window.addEventListener('keydown', handleKeydown);
    return () => window.removeEventListener('keydown', handleKeydown);
  });
</script>

<div class="app-shell">
  <aside class="sidebar">
    <a class="brand" href="/overview" onclick={(event) => navigateFromLink(event, 'overview')} aria-label={tr('ExoRoute overview')}>
      <span class="brand-mark"><FlyingFishLogo size={24} variant="mark" /></span>
      <span class="brand-word">exo<span>route</span></span>
      <span class="brand-version">0.1</span>
    </a>

    <div class="workspace-label">{tr('WORKSPACE')}</div>
    <nav class="primary-nav" aria-label={tr('Main navigation')}>
      {#each navigation as item}
        <a class:active={currentPage === item.id} class="nav-item" href={`/${item.id}`} onclick={(event) => navigateFromLink(event, item.id)}>
          <svelte:component this={item.icon} size={18} strokeWidth={1.8} />
          <span>{tr(item.label)}</span>
          {#if item.id === 'providers' && providerCount}<span class="nav-count">{providerCount}</span>{/if}
        </a>
      {/each}
    </nav>

    <div class="sidebar-spacer"></div>
    <div class="sidebar-status">
      <span class="status-light" class:online={gateway.state === 'loaded'} class:offline={gateway.state === 'error'}></span>
      <div><strong>{tr(gateway.state === 'error' ? 'Gateway unavailable' : 'Gateway connection')}</strong><small>{tr(gateway.state === 'error' ? 'Check API connection' : 'Local instance')}</small></div>
      <button class="icon-button refresh-button" aria-label={tr('Refresh current page')} onclick={onRefresh}><RefreshCw size={15} /></button>
    </div>
    <div class="sidebar-footer"><span class="avatar">ER</span><span class="profile-copy"><strong>{tr('ExoRoute Admin')}</strong><small>{tr('Local workspace')}</small></span><button class="icon-button sign-out-button" aria-label={tr('Sign out')} title={tr('Sign out')} disabled={signingOut} onclick={signOut}>{#if signingOut}<span class="auth-bootstrap-spinner"></span>{:else}<LogOut size={15} />{/if}</button></div>
    {#if signOutError}<p class="sidebar-sign-out-error" role="alert">{signOutError}</p>{/if}
  </aside>

  <main class="main-area">
    <header class="topbar">
      <div class="topbar-left">
        <a class="mobile-brand" href="/overview" onclick={(event) => navigateFromLink(event, 'overview')} aria-label={tr('ExoRoute overview')}>
          <span class="brand-mark"><FlyingFishLogo size={22} variant="mark" /></span>
          <span class="brand-word">exo<span>route</span></span>
        </a>
        <div class="breadcrumbs"><span>{tr('Workspace')}</span><span class="crumb-slash">/</span><strong>{title}</strong></div>
      </div>
      <div class="topbar-actions">
        <div class="preference-controls" aria-label={tr('Language and appearance')}>
          <LanguageSelect locale={preferences.locale} {tr} onLocaleChange={preferences.setLocale} />
          <button class="preference-button theme-button" aria-label={tr(preferences.theme === 'light' ? 'Switch to dark mode' : 'Switch to light mode')} title={tr(preferences.theme === 'light' ? 'Switch to dark mode' : 'Switch to light mode')} aria-pressed={preferences.theme === 'dark'} onclick={preferences.toggleTheme}>{#if preferences.theme === 'light'}<Moon size={15} />{:else}<Sun size={15} />{/if}</button>
        </div>
        <div class="gateway-pill"><span class="status-light" class:online={gateway.state === 'loaded'} class:offline={gateway.state === 'error'}></span><span>{tr(gateway.state === 'error' ? 'Disconnected' : 'Gateway')}</span><code>{gateway.address}</code></div>
        <button class="icon-button mobile-menu-btn" aria-label={tr('Main navigation')} onclick={() => mobileMoreOpen = true}><Menu size={18} /></button>
      </div>
    </header>

    <div class="content-wrap" class:overview-wrap={currentPage === 'overview'}>
      <slot />
      <footer class="page-footer"><span><FlyingFishLogo size={13} variant="monochrome" />{tr('Built for the request path.')}</span><span>ExoRoute <b>·</b> {tr('lightweight by design')}</span></footer>
    </div>
  </main>

  <nav class="mobile-bottom-nav" aria-label={tr('Main navigation')}>
    {#each mobileNavItems as item}
      {#if item.id === 'more'}
        <button
          type="button"
          class="mobile-nav-item"
          class:active={isMoreActive}
          aria-expanded={mobileMoreOpen}
          aria-label={tr('More')}
          onclick={() => mobileMoreOpen = !mobileMoreOpen}
        >
          <div class="mobile-nav-icon-wrap">
            <Ellipsis size={20} />
            {#if isMoreActive}<span class="mobile-nav-dot"></span>{/if}
          </div>
          <span>{tr('More')}</span>
        </button>
      {:else}
        <a
          class="mobile-nav-item"
          class:active={currentPage === item.id}
          href={`/${item.id}`}
          onclick={(event) => { mobileMoreOpen = false; navigateFromLink(event, item.id); }}
        >
          <div class="mobile-nav-icon-wrap">
            <svelte:component this={item.icon} size={20} strokeWidth={currentPage === item.id ? 2.2 : 1.8} />
            {#if item.id === 'providers' && providerCount}<span class="mobile-badge">{providerCount}</span>{/if}
          </div>
          <span>{tr(item.label)}</span>
        </a>
      {/if}
    {/each}
  </nav>

  {#if mobileMoreOpen}
    <div
      class="mobile-sheet-backdrop"
      role="presentation"
      onclick={() => mobileMoreOpen = false}
    ></div>
    <div
      class="mobile-sheet"
      role="dialog"
      aria-modal="true"
      aria-label={tr('More')}
    >
      <div class="mobile-sheet-handle-bar">
        <span class="mobile-sheet-handle"></span>
      </div>
      <div class="mobile-sheet-header">
        <div class="mobile-sheet-title">
          <span class="brand-mark"><FlyingFishLogo size={20} variant="mark" /></span>
          <strong>{tr('Workspace')}</strong>
        </div>
        <button
          type="button"
          class="icon-button mobile-sheet-close"
          aria-label={tr('Close menu')}
          onclick={() => mobileMoreOpen = false}
        >
          <X size={18} />
        </button>
      </div>

      <div class="mobile-sheet-content">
        <div class="mobile-sheet-section-title">{tr('WORKSPACE')}</div>
        <div class="mobile-sheet-nav-grid">
          {#each navigation as item}
            <a
              class="mobile-sheet-nav-item"
              class:active={currentPage === item.id}
              href={`/${item.id}`}
              onclick={(event) => { mobileMoreOpen = false; navigateFromLink(event, item.id); }}
            >
              <svelte:component this={item.icon} size={18} strokeWidth={1.8} />
              <span>{tr(item.label)}</span>
              {#if item.id === 'providers' && providerCount}<span class="nav-count">{providerCount}</span>{/if}
            </a>
          {/each}
        </div>

        <div class="mobile-sheet-status">
          <span class="status-light" class:online={gateway.state === 'loaded'} class:offline={gateway.state === 'error'}></span>
          <div class="mobile-sheet-status-info">
            <strong>{tr(gateway.state === 'error' ? 'Gateway unavailable' : 'Gateway connection')}</strong>
            <small>{gateway.address}</small>
          </div>
          <button class="icon-button refresh-button" aria-label={tr('Refresh current page')} onclick={() => { mobileMoreOpen = false; onRefresh(); }}>
            <RefreshCw size={15} />
          </button>
        </div>

        <div class="mobile-sheet-footer">
          <span class="avatar">ER</span>
          <div class="profile-copy">
            <strong>{tr('ExoRoute Admin')}</strong>
            <small>{tr('Local workspace')}</small>
          </div>
          <button class="icon-button sign-out-button" aria-label={tr('Sign out')} title={tr('Sign out')} disabled={signingOut} onclick={signOut}>
            {#if signingOut}<span class="auth-bootstrap-spinner"></span>{:else}<LogOut size={15} />{/if}
          </button>
        </div>
        {#if signOutError}<p class="sidebar-sign-out-error" role="alert">{signOutError}</p>{/if}
      </div>
    </div>
  {/if}
</div>
