<script lang="ts">
  import { onMount } from 'svelte';
  import DashboardShell from './components/DashboardShell.svelte';
  import LoginPage from './components/LoginPage.svelte';
  import ProviderOAuthCallbackFlow from './features/providers/ProviderOAuthCallbackFlow.svelte';
  import DocsShell from './features/docs/DocsShell.svelte';
  import DocsHomePage from './features/docs/DocsHomePage.svelte';
  import DocsQuickstartPage from './features/docs/DocsQuickstartPage.svelte';
  import DocsIntegrationsPage from './features/docs/DocsIntegrationsPage.svelte';
  import DocsReferencePage from './features/docs/DocsReferencePage.svelte';
  import type { ComponentType } from 'svelte';
  import { api, getAdminAccessToken, notifyAdminLogout, setAdminAccessToken, type AdminAccessResult, type AdminLoginResult } from './lib/api';
  import { createSessionRetryCountdown, restoreSessionOnce } from './lib/auth-flow';
  import { featureComponentLoader } from './lib/feature-registry';
  import { getStoredLocale, saveLocale, t, type Locale } from './lib/i18n';
  import { isDocsPage, pageAfterLogin, pageFromPath, pagePaths, type DashboardPage, type DocsPage, type FeatureActionRequest, type Page } from './lib/navigation';
  import type { Translate } from './lib/format';
  import { getStoredSidebarCollapsed, saveSidebarCollapsed } from './lib/sidebar';
  import { applyTheme, getStoredTheme, type Theme } from './lib/theme';
  type ConnectionState = 'idle' | 'loading' | 'loaded' | 'error';

  function handleConnectionChange(state: ConnectionState): void {
    connectionState = state;
  }

  function handleProviderCountChange(count: number): void {
    providerCount = count;
  }

  const pageTitles: Record<Page, string> = {
    login: 'Sign in',
    overview: 'Overview',
    providers: 'Providers',
    combos: 'Combos',
    quota: 'Quota',
    requests: 'Requests',
    statistics: 'Statistics',
    'api-keys': 'Gateway API keys',
    settings: 'Settings',
    chat: 'Chat',
    docs: 'Documentation',
    'docs-quickstart': 'Quickstart',
    'docs-integrations': 'Integrations',
    'docs-reference': 'API reference',
  };

  let locale: Locale = 'en';
  let theme: Theme = typeof window !== 'undefined' ? getStoredTheme() : 'light';
  let sidebarCollapsed = typeof window !== 'undefined' ? getStoredSidebarCollapsed() : false;
  let tr: Translate;
  let currentPage: Page = 'login';
  let authBootstrap: 'checking' | 'ready' | 'unavailable' = typeof window === 'undefined' ? 'ready' : 'checking';
  let sessionCheckStatus: 'checking' | 'rate_limited' | 'unavailable' | null = null;
  let sessionRetryAfterSeconds = 0;
  let retryAuthBootstrap: () => Promise<void> = async () => {};
  let mustChangePassword = false;
  let currentPassword = '';
  let connectionState: ConnectionState = 'idle';
  let gatewayAddress = typeof window === 'undefined'
    ? '127.0.0.1:8686'
    : `${window.location.hostname || '127.0.0.1'}:${window.location.port || '8686'}`;
  let providerCount = 0;
  let refreshKey = 0;
  let nextActionId = 0;
  let actionRequest: FeatureActionRequest | null = null;
  let activeFeature: ComponentType | null = null;
  let activeFeaturePage: DashboardPage | null = null;
  let activeFeatureRefreshKey = -1;
  let featureLoadError: DashboardPage | null = null;
  let authVersion = 0;
  let providerOAuthNotice = '';
  let providerOAuthNoticeTone: 'info' | 'success' | 'error' = 'info';

  $: tr = (key, vars) => t(locale, key, vars);
  $: preferences = { locale, theme, setLocale: changeLocale, toggleTheme };
  $: gateway = { state: connectionState, address: gatewayAddress };
  $: pageTitle = tr(mustChangePassword && currentPage === 'login' ? 'Set a new admin password' : pageTitles[currentPage]);
  $: if (authBootstrap !== 'checking' && dashboardPage(currentPage) && (activeFeaturePage !== currentPage || activeFeatureRefreshKey !== refreshKey)) {
    activeFeaturePage = null;
    activeFeature = null;
    featureLoadError = null;
    activeFeatureRefreshKey = refreshKey;
    void loadFeatureComponent(currentPage);
  }

  async function loadFeatureComponent(page: DashboardPage): Promise<void> {
    const requestedPage = page;
    const requestedRefreshKey = refreshKey;
    try {
      const module = await featureComponentLoader(requestedPage)();
      if (currentPage !== requestedPage || refreshKey !== requestedRefreshKey) return;
      activeFeature = (module as { default: ComponentType }).default;
      activeFeaturePage = requestedPage;
      activeFeatureRefreshKey = requestedRefreshKey;
    } catch {
      if (currentPage !== requestedPage || refreshKey !== requestedRefreshKey) return;
      // A failed bundle import must stay retryable: keep the page out of the
      // loaded state so the render shows an error with a Retry action instead
      // of an unexplained loading screen that never resolves.
      activeFeature = null;
      activeFeaturePage = null;
      featureLoadError = requestedPage;
    }
  }

  function toggleTheme(): void {
    theme = theme === 'light' ? 'dark' : 'light';
    applyTheme(theme);
  }

  function toggleSidebar(): void {
    sidebarCollapsed = !sidebarCollapsed;
    saveSidebarCollapsed(sidebarCollapsed);
  }

  function changeLocale(nextLocale: Locale): void {
    locale = nextLocale;
    document.documentElement.lang = nextLocale;
    document.documentElement.dir = nextLocale === 'ar' ? 'rtl' : 'ltr';
    saveLocale(nextLocale);
  }

  function updateGatewayAddress(hostValue: unknown, portValue: unknown): void {
    const host = typeof hostValue === 'string' && hostValue.trim()
      ? hostValue.trim()
      : typeof window !== 'undefined' ? window.location.hostname || '127.0.0.1' : '127.0.0.1';
    const port = typeof portValue === 'number' || typeof portValue === 'string'
      ? String(portValue)
      : typeof window !== 'undefined' ? window.location.port || '8686' : '8686';
    gatewayAddress = `${host}:${port}`;
  }

  function dashboardPage(page: Page): page is DashboardPage {
    return page !== 'login' && !isDocsPage(page);
  }

  /**
   * Where an authenticated visitor belongs when they reach the login route.
   * Null when no session is active, or when the session must change its
   * password first — the login route is the right screen for both.
   */
  function signedInDestination(): DashboardPage | null {
    return getAdminAccessToken() && !mustChangePassword ? pageAfterLogin(window.location.search) : null;
  }

  function redirectToLogin(returnTo: DashboardPage): void {
    currentPage = 'login';
    actionRequest = null;
    connectionState = 'idle';
    const target = `${pagePaths.login}?next=${encodeURIComponent(pagePaths[returnTo])}`;
    if (`${window.location.pathname}${window.location.search}` !== target) window.history.replaceState(null, '', target);
  }

  // The docs shell is public, so its entry links point at the login route. A
  // reader who already holds a session goes to the dashboard instead: the login
  // route redirects there anyway, and skipping the form avoids a dead end.
  function leaveDocs(): void {
    const destination = signedInDestination();
    if (destination) {
      navigateTo(destination);
      return;
    }
    currentPage = 'login';
    window.history.pushState(null, '', pagePaths.login);
  }

  function navigateToDocs(page: DocsPage): void {
    const path = pagePaths[page];
    if (window.location.pathname !== path) window.history.pushState(null, '', path);
    currentPage = page;
    actionRequest = null;
    connectionState = 'idle';
  }

  function navigateTo(page: DashboardPage, updatePath = true): void {
    if (!getAdminAccessToken()) {
      redirectToLogin(page);
      return;
    }
    const path = pagePaths[page];
    if (updatePath && window.location.pathname !== path) window.history.pushState(null, '', path);
    currentPage = page;
    actionRequest = null;
    connectionState = 'idle';
  }

  function requestCreate(page: DashboardPage): void {
    if (!getAdminAccessToken()) {
      redirectToLogin(page);
      return;
    }
    const path = pagePaths[page];
    if (window.location.pathname !== path) window.history.pushState(null, '', path);
    currentPage = page;
    connectionState = 'idle';
    actionRequest = { id: ++nextActionId, page };
  }

  function refreshCurrentPage(): void {
    actionRequest = null;
    connectionState = 'idle';
    refreshKey += 1;
  }

  /** Recovers from a route bundle the browser memoized as failed: only a full
   * document load re-fetches the chunk. */
  function reloadDashboard(): void {
    window.location.reload();
  }

  function onLogin(result: AdminLoginResult, enteredPassword: string): void {
    setAdminAccessToken(result.access_token, result.access_expires_in_seconds);
    if (result.must_change_password) {
      mustChangePassword = true;
      currentPassword = enteredPassword;
      currentPage = 'login';
      if (window.location.pathname !== pagePaths.login) window.history.replaceState(null, '', pagePaths.login);
      return;
    }
    mustChangePassword = false;
    currentPassword = '';
    authVersion += 1;
    navigateTo(pageAfterLogin(window.location.search));
  }

  function onPasswordChanged(result: AdminAccessResult): void {
    setAdminAccessToken(result.access_token, result.access_expires_in_seconds);
    mustChangePassword = false;
    currentPassword = '';
    authVersion += 1;
    navigateTo(pageAfterLogin(window.location.search));
  }

  function handleAuthenticationReset(): void {
    notifyAdminLogout();
    mustChangePassword = false;
    currentPassword = '';
    redirectToLogin('settings');
  }

  async function signOut(): Promise<void> {
    await api.adminLogout();
    mustChangePassword = false;
    currentPassword = '';
    redirectToLogin('overview');
  }

  onMount(() => {
    locale = getStoredLocale();
    document.documentElement.lang = locale;
    document.documentElement.dir = locale === 'ar' ? 'rtl' : 'ltr';
    theme = getStoredTheme();
    applyTheme(theme);
    sidebarCollapsed = getStoredSidebarCollapsed();

    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const handleSystemThemeChange = (e: MediaQueryListEvent): void => {
      try {
        if (!localStorage.getItem('exoroute.theme')) {
          theme = e.matches ? 'dark' : 'light';
          applyTheme(theme);
        }
      } catch {}
    };
    mediaQuery.addEventListener('change', handleSystemThemeChange);

    const syncWithLocation = (): void => {
      if (authBootstrap === 'checking') return;
      const page = pageFromPath(window.location.pathname);
      if (isDocsPage(page)) {
        currentPage = page;
        actionRequest = null;
        connectionState = 'idle';
        return;
      }
      if (page === 'login') {
        const destination = signedInDestination();
        if (destination) {
          if (window.location.pathname !== pagePaths[destination]) window.history.replaceState(null, '', pagePaths[destination]);
          navigateTo(destination, false);
          return;
        }
        currentPage = 'login';
        actionRequest = null;
        connectionState = 'idle';
        if (window.location.pathname !== pagePaths.login) {
          window.history.replaceState(null, '', `${pagePaths.login}${window.location.search}`);
        }
        return;
      }
      if (!getAdminAccessToken()) {
        redirectToLogin(page);
        return;
      }
      if (window.location.pathname !== pagePaths[page] || window.location.hash) {
        window.history.replaceState(null, '', pagePaths[page]);
      }
      currentPage = page;
      actionRequest = null;
    };

    const handleAuthRequired = (): void => {
      setAdminAccessToken('');
      mustChangePassword = false;
      currentPassword = '';
      if (dashboardPage(currentPage)) redirectToLogin(currentPage);
      else currentPage = 'login';
    };

    const handlePasswordChangeRequired = (): void => {
      mustChangePassword = true;
      currentPassword = '';
      if (dashboardPage(currentPage)) redirectToLogin(currentPage);
      else currentPage = 'login';
    };

    window.addEventListener('popstate', syncWithLocation);
    window.addEventListener('exoroute:auth-required', handleAuthRequired);
    window.addEventListener('exoroute:password-change-required', handlePasswordChangeRequired);
    let disposed = false;
    const sessionRetryCountdown = createSessionRetryCountdown((seconds) => {
      sessionRetryAfterSeconds = seconds;
    });
    const initializeAuthentication = async (manualRetry = false): Promise<void> => {
      if (manualRetry && sessionRetryAfterSeconds > 0) return;
      if (manualRetry) sessionCheckStatus = 'checking';
      else authBootstrap = 'checking';
      const outcome = await restoreSessionOnce();
      if (disposed) return;
      if (outcome.kind === 'unavailable') {
        authBootstrap = 'unavailable';
        sessionCheckStatus = outcome.checkStatus;
        sessionRetryCountdown.start(outcome.retryAfterSeconds);
        mustChangePassword = false;
        currentPassword = '';
        syncWithLocation();
        return;
      }
      sessionRetryCountdown.stop();
      sessionRetryAfterSeconds = 0;
      sessionCheckStatus = null;
      mustChangePassword = outcome.mustChangePassword;
      currentPassword = '';
      authBootstrap = 'ready';
      if (outcome.authenticated && !mustChangePassword) authVersion += 1;
      if (mustChangePassword && window.location.pathname !== pagePaths.login) {
        window.history.replaceState(null, '', pagePaths.login);
      }
      syncWithLocation();
    };
    retryAuthBootstrap = () => initializeAuthentication(true);
    void initializeAuthentication();
    return () => {
      disposed = true;
      sessionRetryCountdown.stop();
      mediaQuery.removeEventListener('change', handleSystemThemeChange);
      window.removeEventListener('popstate', syncWithLocation);
      window.removeEventListener('exoroute:auth-required', handleAuthRequired);
      window.removeEventListener('exoroute:password-change-required', handlePasswordChangeRequired);
    };
  });
</script>

<svelte:head>
  <title>{pageTitle} · ExoRoute</title>
  <meta name="description" content={tr('ExoRoute gateway administration')} />
  <meta name="referrer" content="no-referrer" />
</svelte:head>

<ProviderOAuthCallbackFlow
  {tr}
  {mustChangePassword}
  {authVersion}
  bind:notice={providerOAuthNotice}
  bind:noticeTone={providerOAuthNoticeTone}
  onNavigate={navigateTo}
  onRefresh={refreshCurrentPage}
/>

{#if authBootstrap === 'checking'}
  <main class="auth-bootstrap session-check-screen" role="status"><section class="session-check-card"><span class="auth-bootstrap-spinner"></span><p>{tr('Checking admin session…')}</p></section></main>
{:else if isDocsPage(currentPage)}
  <DocsShell page={currentPage} {locale} {preferences} onNavigate={navigateToDocs} onExitDocs={leaveDocs}>
    {#if currentPage === 'docs'}
      <DocsHomePage {locale} onNavigate={navigateToDocs} />
    {:else if currentPage === 'docs-quickstart'}
      <DocsQuickstartPage {locale} />
    {:else if currentPage === 'docs-integrations'}
      <DocsIntegrationsPage {locale} />
    {:else}
      <DocsReferencePage {locale} />
    {/if}
  </DocsShell>
{:else if currentPage === 'login'}
  <LoginPage {tr} {preferences} {mustChangePassword} {currentPassword} {onLogin} onPasswordChanged={onPasswordChanged} sessionCheckStatus={authBootstrap === 'unavailable' ? sessionCheckStatus : null} {sessionRetryAfterSeconds} onRetrySessionCheck={retryAuthBootstrap} />
{:else}
  <DashboardShell currentPage={currentPage} title={pageTitle} {tr} {preferences} {gateway} {providerCount} collapsed={sidebarCollapsed} onToggleCollapse={toggleSidebar} onNavigate={navigateTo} onRefresh={refreshCurrentPage} onSignOut={signOut}>
    {#if providerOAuthNotice}
      <div class="provider-oauth-callback-banner" class:success={providerOAuthNoticeTone === 'success'} class:error={providerOAuthNoticeTone === 'error'} role={providerOAuthNoticeTone === 'error' ? 'alert' : 'status'}>
        <span>{providerOAuthNotice}</span>
        <button type="button" class="icon-button" aria-label={tr('Dismiss notification')} onclick={() => providerOAuthNotice = ''}>×</button>
      </div>
    {/if}
    {#if activeFeature && activeFeaturePage === currentPage && activeFeatureRefreshKey === refreshKey}
        {#if currentPage === 'overview'}
          <svelte:component this={activeFeature} {tr} {locale} onNavigate={navigateTo} onCreateRequest={requestCreate} onConnectionChange={handleConnectionChange} onProviderCountChange={handleProviderCountChange} onGatewayAddressChange={updateGatewayAddress} />
        {:else if currentPage === 'providers'}
          <svelte:component this={activeFeature} {tr} {locale} {actionRequest} onConnectionChange={handleConnectionChange} onProviderCountChange={handleProviderCountChange} />
        {:else if currentPage === 'combos'}
          <svelte:component this={activeFeature} {tr} {actionRequest} onNavigate={navigateTo} onConnectionChange={handleConnectionChange} onProviderCountChange={handleProviderCountChange} />
        {:else if currentPage === 'quota'}
          <svelte:component this={activeFeature} {tr} {locale} onNavigate={navigateTo} onConnectionChange={handleConnectionChange} onProviderCountChange={handleProviderCountChange} />
        {:else if currentPage === 'requests'}
          <svelte:component this={activeFeature} {tr} {locale} onConnectionChange={handleConnectionChange} />
        {:else if currentPage === 'statistics'}
          <svelte:component this={activeFeature} {tr} {locale} onConnectionChange={handleConnectionChange} />
        {:else if currentPage === 'api-keys'}
          <svelte:component this={activeFeature} {tr} {locale} {actionRequest} onConnectionChange={handleConnectionChange} />
        {:else if currentPage === 'settings'}
          <svelte:component this={activeFeature} {tr} {locale} onNavigate={navigateTo} onConnectionChange={handleConnectionChange} onAuthenticationReset={handleAuthenticationReset} onGatewayAddressChange={updateGatewayAddress} />
        {:else if currentPage === 'chat'}
          <svelte:component this={activeFeature} {tr} {locale} onConnectionChange={handleConnectionChange} />
        {/if}
      {:else if featureLoadError === currentPage}
        <main class="auth-bootstrap session-check-screen" role="alert">
          <section class="session-check-card">
            <h1>{tr('Could not load this page.')}</h1>
            <p>{tr('Check the server, then try again.')}</p>
            <button class="primary-button" type="button" onclick={refreshCurrentPage}>{tr('Retry')}</button>
            <button class="secondary-button" type="button" onclick={reloadDashboard}>{tr('Reload page')}</button>
          </section>
        </main>
      {:else}
        <main class="auth-bootstrap session-check-screen" role="status"><section class="session-check-card"><span class="auth-bootstrap-spinner"></span><p>{tr('Loading {page}…', { page: pageTitles[currentPage] })}</p></section></main>
      {/if}
  </DashboardShell>
{/if}
