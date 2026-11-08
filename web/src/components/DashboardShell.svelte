<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    Activity, AlertTriangle, ArrowUpCircle, BookOpen, Boxes, Ellipsis, Gauge, KeyRound, Layers3, LogOut, Menu, MessageSquare, Moon, PanelLeftClose, PanelLeftOpen, RefreshCw, Settings2, Sun, X,
  } from '@lucide/svelte';
  import FlyingFishLogo from './FlyingFishLogo.svelte';
  import LanguageSelect from './LanguageSelect.svelte';
  import type { DashboardPage } from '../lib/navigation';
  import type { Locale } from '../lib/i18n';
  import type { Translate } from '../lib/format';
  import { checkForReleaseUpdate, dismissRelease, type ReleaseUpdate } from '../lib/release-check';
  import {
    checkFailureRate,
    dismissFailureAlert,
    FAILURE_RATE_POLL_MS,
    failureAlertMessage,
    type FailureAlert,
  } from '../lib/failure-rate';
  import { DASHBOARD_VERSION } from '../lib/version';
  import type { ThemeId, ThemeScheme } from '../lib/theme';

  type ConnectionState = 'idle' | 'loading' | 'loaded' | 'error';
  interface GatewayStatus { state: ConnectionState; address: string }
  interface Preferences {
    locale: Locale;
    theme: ThemeId;
    scheme: ThemeScheme;
    setLocale: (locale: Locale) => void;
    toggleTheme: () => void;
  }

  export let currentPage: DashboardPage;
  export let title: string;
  export let tr: Translate;
  export let preferences: Preferences;
  export let gateway: GatewayStatus;
  export let providerCount = 0;
  /** Icon-only rail state, owned by the app so it can persist across pages. */
  export let collapsed = false;
  export let onToggleCollapse: () => void = () => {};
  export let onNavigate: (page: DashboardPage) => void;
  export let onRefresh: () => void;
  export let onSignOut: () => Promise<void>;

  let signingOut = false;
  let signOutError = '';
  let mobileMoreOpen = false;
  // Best-effort release notice: the check runs once per shell mount and every
  // failure path leaves this null, so the shell renders exactly as before.
  let updateNotice: ReleaseUpdate | null = null;
  // Same best-effort contract for the failure-rate banner; polled once a
  // minute while the tab is visible.
  let failureAlert: FailureAlert | null = null;
  let failurePoller: number | null = null;

  const navigation = [
    { id: 'overview', label: 'Overview', icon: Gauge },
    { id: 'providers', label: 'Providers', icon: Boxes },
    { id: 'combos', label: 'Combos', icon: Layers3 },
    { id: 'quota', label: 'Quota', icon: Gauge },
    { id: 'api-keys', label: 'API keys', icon: KeyRound },
    { id: 'requests', label: 'Requests', icon: Activity },
    { id: 'chat', label: 'Chat', icon: MessageSquare },
    { id: 'settings', label: 'Settings', icon: Settings2 },
  ] as const;

  /** Workspace pages precede Docs in the sidebar; Docs closes the list. */
  const docsNavItem = { id: 'docs', label: 'Docs', icon: BookOpen };

  const mobileNavItems = [
    { id: 'overview', label: 'Overview', icon: Gauge },
    { id: 'providers', label: 'Providers', icon: Boxes },
    { id: 'combos', label: 'Combos', icon: Layers3 },
    { id: 'requests', label: 'Requests', icon: Activity },
    { id: 'more', label: 'More', icon: Ellipsis },
  ] as const;

  $: isMoreActive = ['api-keys', 'quota', 'settings', 'chat'].includes(currentPage);

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

  function dismissUpdateNotice(): void {
    if (updateNotice) dismissRelease(updateNotice.latest);
    updateNotice = null;
  }

  async function refreshFailureAlert(): Promise<void> {
    if (document.hidden) return;
    failureAlert = await checkFailureRate();
  }

  function dismissFailureNotice(): void {
    if (failureAlert) dismissFailureAlert(failureAlert.incidentId);
    failureAlert = null;
  }

  onMount(() => {
    window.addEventListener('keydown', handleKeydown);
    void checkForReleaseUpdate().then((update) => {
      updateNotice = update;
    });
    void refreshFailureAlert();
    failurePoller = window.setInterval(() => { void refreshFailureAlert(); }, FAILURE_RATE_POLL_MS);
    return () => {
      window.removeEventListener('keydown', handleKeydown);
      if (failurePoller !== null) window.clearInterval(failurePoller);
      failurePoller = null;
    };
  });
</script>

<div class="app-shell" class:sidebar-collapsed={collapsed}>
  <aside class="sidebar" class:collapsed>
    <div class="sidebar-brand-row">
      <a class="brand" href="/overview" onclick={(event) => navigateFromLink(event, 'overview')} aria-label={tr('ExoRoute overview')}>
        <span class="brand-mark"><FlyingFishLogo size={24} variant="mark" /></span>
        <span class="brand-word">exo<span>route</span></span>
        <span class="brand-version">{DASHBOARD_VERSION}</span>
      </a>
      <button
        type="button"
        class="icon-button sidebar-toggle"
        aria-label={tr(collapsed ? 'Expand sidebar' : 'Collapse sidebar')}
        title={tr(collapsed ? 'Expand sidebar' : 'Collapse sidebar')}
        onclick={onToggleCollapse}
      >
        {#if collapsed}<PanelLeftOpen size={16} />{:else}<PanelLeftClose size={16} />{/if}
      </button>
    </div>

    <div class="workspace-label">{tr('WORKSPACE')}</div>
    <nav class="primary-nav" aria-label={tr('Main navigation')}>
      {#each navigation as item}
        <a class:active={currentPage === item.id} class="nav-item" href={`/${item.id}`} aria-label={tr(item.label)} title={tr(item.label)} onclick={(event) => navigateFromLink(event, item.id)}>
          <svelte:component this={item.icon} size={18} strokeWidth={1.8} />
          <span>{tr(item.label)}</span>
          {#if item.id === 'providers' && providerCount}<span class="nav-count">{providerCount}</span>{/if}
        </a>
      {/each}
      <a class="nav-item docs-nav-item" href="/docs" aria-label={tr(docsNavItem.label)} title={tr(docsNavItem.label)}>
        <svelte:component this={docsNavItem.icon} size={18} strokeWidth={1.8} />
        <span>{tr(docsNavItem.label)}</span>
      </a>
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
      <div class="topbar-inner">
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
            <button class="preference-button theme-button" aria-label={tr(preferences.scheme === 'light' ? 'Switch to dark mode' : 'Switch to light mode')} title={tr(preferences.scheme === 'light' ? 'Switch to dark mode' : 'Switch to light mode')} aria-pressed={preferences.scheme === 'dark'} onclick={preferences.toggleTheme}>{#if preferences.scheme === 'light'}<Moon size={15} />{:else}<Sun size={15} />{/if}</button>
          </div>
          <div class="gateway-pill"><span class="status-light" class:online={gateway.state === 'loaded'} class:offline={gateway.state === 'error'}></span><span>{tr(gateway.state === 'error' ? 'Disconnected' : 'Gateway')}</span><code>{gateway.address}</code></div>
          <button class="icon-button mobile-menu-btn" aria-label={tr('Main navigation')} onclick={() => mobileMoreOpen = true}><Menu size={18} /></button>
        </div>
      </div>
    </header>

      {#if updateNotice}
        <!-- flex-shrink: 0 keeps the notice intact inside the flex column, the
             same invariant the page sections rely on (tests/requests-layout).
             Styles live in styles/dashboard-surfaces.css with the other shell
             surfaces so base.css stays inside its source budget. -->
      <div class="update-notice" role="status">
        <span class="update-notice-icon" aria-hidden="true"><ArrowUpCircle size={15} /></span>
        <p class="update-notice-text">
          <strong>{tr('Update available')}</strong>
          <span>{tr('ExoRoute {latest} is available. You are running {current}.', { latest: updateNotice.latest, current: DASHBOARD_VERSION })}</span>
        </p>
        <a class="update-notice-link" href={updateNotice.url} target="_blank" rel="noopener noreferrer">{tr('View release')}</a>
        <button type="button" class="icon-button update-notice-dismiss" aria-label={tr('Dismiss notification')} onclick={dismissUpdateNotice}>×</button>
      </div>
    {/if}

    {#if failureAlert}
      <!-- Same shell slot as the update notice; amber tone so the two never
           read as the same kind of message. -->
      <div class="failure-notice" role="alert">
        <span class="failure-notice-icon" aria-hidden="true"><AlertTriangle size={15} /></span>
        <p class="failure-notice-text">
          <strong>{tr('High failure rate')}</strong>
          <span>{failureAlertMessage(failureAlert, preferences.locale)}</span>
        </p>
        <a
          class="failure-notice-link"
          href="/requests"
          onclick={(event) => {
            const modifier = event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey;
            if (modifier) return;
            event.preventDefault();
            dismissFailureNotice();
            onNavigate('requests');
          }}
        >{tr('Requests')}</a>
        <button type="button" class="icon-button update-notice-dismiss" aria-label={tr('Dismiss notification')} onclick={dismissFailureNotice}>×</button>
      </div>
    {/if}

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
          <a class="mobile-sheet-nav-item" href="/docs" onclick={() => { mobileMoreOpen = false; }}>
            <svelte:component this={docsNavItem.icon} size={18} strokeWidth={1.8} />
            <span>{tr(docsNavItem.label)}</span>
          </a>
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
