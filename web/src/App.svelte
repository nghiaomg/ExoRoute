<script lang="ts">
  import Button from "./components/ui/Button.svelte";
  import { onMount } from 'svelte';
  import DashboardAppShell from './components/layout/DashboardAppShell.svelte';
  import LoginPage from './features/auth/LoginPage.svelte';
  import KeyPortalPage from './features/key-portal/KeyPortalPage.svelte';
  import { keyPortalApi, type KeyPortalSession } from './lib/api/keyPortal';
  import ProviderOAuthCallbackFlow from './features/providers/ProviderOAuthCallbackFlow.svelte';
  import DocsLayout from './features/docs/DocsLayout.svelte';
  import DocsHomePage from './features/docs/DocsHomePage.svelte';
  import DocsQuickstartPage from './features/docs/DocsQuickstartPage.svelte';
  import DocsIntegrationsPage from './features/docs/DocsIntegrationsPage.svelte';
  import DocsReferencePage from './features/docs/DocsReferencePage.svelte';
  import type { ComponentType } from 'svelte';
  import { api, getAdminAccessToken, notifyAdminLogout, setAdminAccessToken, type AdminAccessResult, type AdminLoginResult } from './lib/api';
  import { createSessionRetryCountdown, restoreSessionOnce } from './lib/auth-flow';
  import { featureComponentLoader } from './lib/feature-registry';
  import { getStoredLocale, saveLocale, t, type Locale } from './lib/i18n';
  import { isDocsPage, pageAfterLogin, pageFromPath, pagePaths, type DashboardPage, type DashboardPageRoute, type DocsPage, type FeatureActionRequest, type Page } from './lib/navigation';
  import type { Translate } from './lib/format';
  import {
    applyThemePreference,
    getStoredThemePreference,
    isThemePreference,
    resolveTheme,
    setStoredThemePreference,
    themeScheme,
    type ThemeId,
    type ThemePreference,
  } from './lib/theme';
  type ConnectionState = 'idle' | 'loading' | 'loaded' | 'error';

  // Pages rendered in the current dashboard shell. Keep the set explicit so a
  // future route can opt into the legacy shell while its feature is migrated.
  const dashboardShellPages = new Set<DashboardPage>(['overview', 'providers', 'combos', 'proxies', 'quota', 'requests', 'chat', 'api-keys', 'settings', 'help']);
  function isApiKeyDetailRoute(page: Page | DashboardPageRoute): boolean {
    return page === 'api-key-detail' || page.startsWith('api-key-detail:');
  }
  $: useDashboardShell = dashboardPage(currentPage) && (dashboardShellPages.has(currentPage as DashboardPage) || isApiKeyDetailRoute(currentPage));

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
    proxies: 'Proxies',
    quota: 'Quota',
    requests: 'Requests',
    'api-keys': 'Gateway API keys',
    'api-key-detail': 'Key details',
    settings: 'Settings',
    chat: 'Chat',
    help: 'Help Center',
    docs: 'Documentation',
    'docs-overview': 'Documentation',
    'docs-quickstart': 'Quickstart',
    'docs-integrations': 'Integrations',
    'docs-reference': 'API reference',
  };

  let locale: Locale = 'en';
  let themePreference: ThemePreference = typeof window !== 'undefined' ? getStoredThemePreference() : 'system';
  let theme: ThemeId = typeof window !== 'undefined' ? resolveTheme(themePreference) : 'light';
  let tr: Translate;
  let currentPage: Page | DashboardPageRoute = 'login';
  let authBootstrap: 'checking' | 'ready' | 'unavailable' = typeof window === 'undefined' ? 'ready' : 'checking';
  let sessionCheckStatus: 'checking' | 'rate_limited' | 'unavailable' | null = null;
  let sessionRetryAfterSeconds = 0;
  let retryAuthBootstrap: () => Promise<void> = async () => {};
  let mustChangePassword = false;
  let keyPortalActive = false;
  let keyPortalSession: KeyPortalSession | null = null;
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
  let activeFeaturePage: DashboardPageRoute | null = null;
  let activeFeatureRefreshKey = -1;
  let featureLoadError: DashboardPageRoute | null = null;
  let authVersion = 0;
  let providerOAuthNotice = '';
  let providerOAuthNoticeTone: 'info' | 'success' | 'error' = 'info';

  $: tr = (key, vars) => t(locale, key, vars);
  $: preferences = { locale, theme, themePreference, scheme: themeScheme(theme), setLocale: changeLocale, toggleTheme, setTheme: changeTheme, setThemePreference: changeThemePreference };
  $: gateway = { state: connectionState, address: gatewayAddress };
  function currentPageTitle(): string {
    if (mustChangePassword && currentPage === 'login') return tr('Set a new admin password');
    if (isApiKeyDetailRoute(currentPage)) return tr('Key details');
    return tr(pageTitles[currentPage as Page]);
  }
  // Keep the reactive dependencies inline: Svelte cannot track reads made
  // inside currentPageTitle(), so `$: pageTitle = currentPageTitle()` would
  // compute once and leave the document title stuck on the login page.
  $: pageTitle = mustChangePassword && currentPage === 'login'
    ? tr('Set a new admin password')
    : isApiKeyDetailRoute(currentPage)
      ? tr('Key details')
      : tr(pageTitles[currentPage as Page]);
  $: if (authBootstrap !== 'checking' && dashboardPage(currentPage) && (activeFeaturePage !== currentPage || activeFeatureRefreshKey !== refreshKey)) {
    activeFeaturePage = null;
    activeFeature = null;
    featureLoadError = null;
    activeFeatureRefreshKey = refreshKey;
    void loadFeatureComponent(currentPage as DashboardPageRoute);
  }

  async function loadFeatureComponent(page: DashboardPageRoute): Promise<void> {
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
    // The quick toggle always makes the opposite effective classic scheme an
    // explicit choice, including when the current preference follows the OS.
    const next: ThemeId = themeScheme(theme) === 'light' ? 'dark' : 'light';
    changeTheme(next);
  }

  function changeTheme(next: ThemeId): void {
    changeThemePreference(next);
  }

  function changeThemePreference(next: ThemePreference): void {
    themePreference = next;
    setStoredThemePreference(next);
    theme = applyThemePreference(next);
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

  function dashboardPage(page: Page | DashboardPageRoute): page is DashboardPageRoute {
    return page !== 'login' && !isDocsPage(page as Page);
  }

  function dashboardPathFor(page: DashboardPageRoute): string {
    return isApiKeyDetailRoute(page)
      ? `/api-keys/${encodeURIComponent(page.slice('api-key-detail:'.length))}`
      : pagePaths[page as DashboardPage];
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

  function navigateTo(page: DashboardPageRoute, updatePath = true): void {
    if (!getAdminAccessToken()) {
      redirectToLogin(isApiKeyDetailRoute(page) ? 'api-keys' : page as DashboardPage);
      return;
    }
    const path = dashboardPathFor(page);
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

  function onKeyPortalLogin(session: KeyPortalSession): void {
    keyPortalSession = session;
    keyPortalActive = true;
    currentPage = 'login';
  }

  async function signOutOfKeyPortal(): Promise<void> {
    try {
      await keyPortalApi.logout();
    } catch {
      // The local view returns to login even if the cookie was already gone.
    }
    keyPortalActive = false;
    keyPortalSession = null;
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
    themePreference = getStoredThemePreference();
    theme = applyThemePreference(themePreference);

    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const handleSystemThemeChange = (e: MediaQueryListEvent): void => {
      if (themePreference === 'system') {
        theme = applyThemePreference('system', e.matches ? 'dark' : 'light');
      }
    };
    const handleThemeStorage = (e: StorageEvent): void => {
      if (e.key !== 'exoroute.theme' && e.key !== null) return;
      themePreference = isThemePreference(e.newValue) ? e.newValue : 'system';
      theme = applyThemePreference(themePreference);
    };
    mediaQuery.addEventListener('change', handleSystemThemeChange);
    window.addEventListener('storage', handleThemeStorage);

    const syncWithLocation = (): void => {
      if (authBootstrap === 'checking') return;
      const page = pageFromPath(window.location.pathname);
      if (isDocsPage(page as Page)) {
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
        redirectToLogin(isApiKeyDetailRoute(page) ? 'api-keys' : page as DashboardPage);
        return;
      }
      if (window.location.pathname !== dashboardPathFor(page as DashboardPageRoute) || window.location.hash) {
        window.history.replaceState(null, '', dashboardPathFor(page as DashboardPageRoute));
      }
      currentPage = page;
      actionRequest = null;
    };

    const handleAuthRequired = (): void => {
      setAdminAccessToken('');
      mustChangePassword = false;
      currentPassword = '';
      if (dashboardPage(currentPage)) redirectToLogin(isApiKeyDetailRoute(currentPage) ? 'api-keys' : currentPage as DashboardPage);
      else currentPage = 'login';
    };

    const handlePasswordChangeRequired = (): void => {
      mustChangePassword = true;
      currentPassword = '';
      if (dashboardPage(currentPage)) redirectToLogin(isApiKeyDetailRoute(currentPage) ? 'api-keys' : currentPage as DashboardPage);
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
      if (!outcome.authenticated) {
        try {
          const portalSession = await keyPortalApi.session();
          if (!disposed && portalSession) {
            keyPortalSession = portalSession;
            keyPortalActive = true;
          }
        } catch {
          keyPortalActive = false;
          keyPortalSession = null;
        }
      }
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
      window.removeEventListener('storage', handleThemeStorage);
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
{:else if isDocsPage(currentPage as Page)}
  <DocsLayout
    page={currentPage as DocsPage}
    {locale}
    {preferences}
    providerCount={providerCount}
    gatewayAddress={gateway.address}
    onNavigate={(target) => {
      if (isDocsPage(target)) navigateToDocs(target);
      else if (dashboardPage(target)) navigateTo(target);
    }}
    onExitDocs={leaveDocs}
  >
    {#if currentPage === 'docs' || currentPage === 'docs-overview'}
      <DocsHomePage {locale} onNavigate={navigateToDocs} />
    {:else if currentPage === 'docs-quickstart'}
      <DocsQuickstartPage {locale} />
    {:else if currentPage === 'docs-integrations'}
      <DocsIntegrationsPage {locale} />
    {:else}
      <DocsReferencePage {locale} />
    {/if}
  </DocsLayout>
{:else if keyPortalActive && keyPortalSession}
  <KeyPortalPage {tr} {locale} session={keyPortalSession} onSignOut={signOutOfKeyPortal} />
{:else if currentPage === 'login'}
  <LoginPage {tr} {preferences} {mustChangePassword} {currentPassword} {onLogin} onPasswordChanged={onPasswordChanged} {onKeyPortalLogin} sessionCheckStatus={authBootstrap === 'unavailable' ? sessionCheckStatus : null} {sessionRetryAfterSeconds} onRetrySessionCheck={retryAuthBootstrap} />
{:else if useDashboardShell}
  <DashboardAppShell page={isApiKeyDetailRoute(currentPage) ? 'api-keys' : currentPage as DashboardPage} {providerCount} gatewayAddress={gateway.address} {tr} {locale} onLocaleChange={changeLocale} onNavigate={(target) => navigateTo(target as DashboardPage)} onSignOut={signOut}>
    {#if providerOAuthNotice}
      <div class="provider-oauth-callback-banner" class:success={providerOAuthNoticeTone === 'success'} class:error={providerOAuthNoticeTone === 'error'} role={providerOAuthNoticeTone === 'error' ? 'alert' : 'status'}>
        <span>{providerOAuthNotice}</span>
        <Button type="button" class="icon-button" aria-label={tr('Dismiss notification')} onclick={() => providerOAuthNotice = ''}>×</Button>
      </div>
    {/if}
    {#if activeFeature && activeFeaturePage === currentPage && activeFeatureRefreshKey === refreshKey}
      {#if currentPage === 'overview'}
        <svelte:component this={activeFeature} {tr} {locale} onNavigate={navigateTo} onCreateRequest={requestCreate} onConnectionChange={handleConnectionChange} onProviderCountChange={handleProviderCountChange} onGatewayAddressChange={updateGatewayAddress} />
      {:else if currentPage === 'providers'}
        <svelte:component this={activeFeature} {tr} {locale} onConnectionChange={handleConnectionChange} onProviderCountChange={handleProviderCountChange} />
      {:else if currentPage === 'combos'}
        <svelte:component this={activeFeature} {tr} {actionRequest} onNavigate={navigateTo} onConnectionChange={handleConnectionChange} onProviderCountChange={handleProviderCountChange} />
      {:else if currentPage === 'proxies'}
        <svelte:component this={activeFeature} {tr} {locale} />
      {:else if currentPage === 'quota'}
        <svelte:component this={activeFeature} {tr} {locale} onNavigate={navigateTo} onConnectionChange={handleConnectionChange} onProviderCountChange={handleProviderCountChange} />
      {:else if currentPage === 'requests'}
        <svelte:component this={activeFeature} {tr} {locale} onConnectionChange={handleConnectionChange} />
      {:else if currentPage === 'settings'}
        <svelte:component this={activeFeature} {tr} {theme} {themePreference} onThemeChange={changeTheme} onThemePreferenceChange={changeThemePreference} onNavigate={navigateTo} onAuthenticationReset={handleAuthenticationReset} />
      {:else if currentPage === 'api-keys' || isApiKeyDetailRoute(currentPage)}
        <svelte:component this={activeFeature} {tr} {locale} page={currentPage} onNavigate={navigateTo} />
      {:else if currentPage === 'help'}
        <svelte:component this={activeFeature} {tr} {locale} onNavigate={navigateTo} />
      {:else if currentPage === 'chat'}
        <svelte:component this={activeFeature} {tr} {locale} onConnectionChange={handleConnectionChange} />
      {/if}
    {:else if featureLoadError === currentPage}
      <section class="course-card surface" role="alert">
        <div class="card-title-row"><h2>{tr('Could not load this page.')}</h2></div>
        <p class="card-subtitle">{tr('Check the server, then try again.')}</p>
        <div class="modal-actions" style="padding-top:14px">
          <Button class="page-action primary" type="button" onclick={refreshCurrentPage}>{tr('Retry')}</Button>
          <Button class="page-action" type="button" onclick={reloadDashboard}>{tr('Reload page')}</Button>
        </div>
      </section>
    {:else}
      <section class="grid min-h-0 grid-cols-[repeat(4,minmax(0,1fr))] gap-[2%] max-nav:grid-cols-2 max-nav:gap-2" role="status" aria-label={tr('Loading {page}…', { page: currentPageTitle() })}>
        {#each [0, 1, 2, 3] as i (i)}<div class="min-w-0 min-h-[152px] rounded-[17px] border border-line bg-surface p-[18px]" style="min-height:132px"></div>{/each}
      </section>
    {/if}
  </DashboardAppShell>
{/if}
