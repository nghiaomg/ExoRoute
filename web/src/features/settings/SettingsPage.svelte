<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Activity, CircleHelp, Cpu, Database, ExternalLink, KeyRound, Radio, RefreshCw, Server, ShieldCheck, SlidersHorizontal, Sparkles } from '@lucide/svelte';
  import { Tabs } from '@ark-ui/svelte/tabs';
  import GatewayError from '../../components/GatewayError.svelte';
  import InlineLoading from '../../components/InlineLoading.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import { api, setAdminAccessToken, type AdminAccessResult } from '../../lib/api';
  import type { DashboardPage } from '../../lib/navigation';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { en as enCatalog } from '../../lib/locales/en';
  import { formatUptime, type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Overview, PublicSettings, UpdateCheckResult } from '../../lib/types';
  import AdminPasswordChangeDialog from './AdminPasswordChangeDialog.svelte';
  import DatabaseBackupPanel from './DatabaseBackupPanel.svelte';
  import GatewayResourceLimitsPanel from './GatewayResourceLimitsPanel.svelte';
  import OperationalSettingsPanel from './OperationalSettingsPanel.svelte';
  import OutputStylesPanel from './OutputStylesPanel.svelte';
  import { createUpdateCheckStore } from './update-check';

  export let tr: Translate;
  export let locale: Locale;
  export let onNavigate: (page: DashboardPage) => void;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  export let onAuthenticationReset: () => void;
  export let onGatewayAddressChange: (host: unknown, port: unknown) => void;

  type SettingsTab = 'server' | 'operational' | 'output-styles' | 'resources' | 'backup';
  let activeTab: SettingsTab = 'server';

  let settings: PublicSettings = {};
  let overview: Overview | null = null;
  let loading = true;
  let errorMessage = '';
  let passwordDialogOpen = false;
  let generation = 0;
  let panelRefreshToken = 0;
  let updateCheck: UpdateCheckResult | null = null;
  let updateCheckLoading = false;
  let updateCheckFailed = false;
  const updateCheckStore = createUpdateCheckStore();

  $: ({ result: updateCheck, loading: updateCheckLoading, failed: updateCheckFailed } = $updateCheckStore);

  async function load(): Promise<void> {
    const requestGeneration = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const [settingsResult, overviewResult] = await Promise.all([api.settings(), api.overview()]);
      if (requestGeneration !== generation) return;
      settings = settingsResult;
      onGatewayAddressChange(settingsResult.host, settingsResult.port);
      overview = overviewResult;
      onConnectionChange('loaded');
      updateCheckStore.check();
    } catch (error) {
      if (requestGeneration !== generation) return;
      errorMessage = localizedError(error, 'Something went wrong while loading this page.', tr);
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) loading = false;
    }
  }

  function checkForUpdates(): void {
    updateCheckStore.check();
  }

  function selectTab(newTab: SettingsTab): void {
    activeTab = newTab;
    try {
      window.history.replaceState(null, '', `#${newTab}`);
    } catch {
      // Ignore in sandboxed contexts
    }
  }

  function configKeyLabel(key: string): string {
    if (!Object.hasOwn(enCatalog, key)) return key;
    return tr(key);
  }

  function passwordUpdated(result: AdminAccessResult): void {
    passwordDialogOpen = false;
    setAdminAccessToken(result.access_token, result.access_expires_in_seconds);
  }

  function databaseImported(): void {
    panelRefreshToken += 1;
    void load();
  }

  onMount(() => {
    try {
      const hash = window.location.hash.replace('#', '');
      if (hash === 'server' || hash === 'operational' || hash === 'output-styles' || hash === 'resources' || hash === 'backup') {
        activeTab = hash;
      }
    } catch {
      // Ignore
    }
    void load();
  });

  onDestroy(() => {
    generation += 1;
  });
</script>

<div class="settings-view">
  <PageHeading title={tr('Settings')} subtitle={tr('Runtime information and public gateway configuration.')} {tr} />

  {#if errorMessage}
    <GatewayError message={errorMessage} {tr} onRetry={load} />
  {:else if loading}
    <InlineLoading label={'Loading {page}…'} {tr} vars={{ page: tr('Settings').toLowerCase() }} />
  {:else}
    <Tabs.Root
      value={activeTab}
      onValueChange={(details) => selectTab(details.value as SettingsTab)}
      class="settings-tabs-root"
    >
      <!-- Modern Segmented Tabs Bar -->
      <Tabs.List class="settings-nav-tabs" aria-label={tr('Settings')}>
        <Tabs.Trigger
          class="settings-nav-tab {activeTab === 'server' ? 'active' : ''}"
          value="server"
        >
          <Server size={15} />
          <span>{tr('Server & Security')}</span>
        </Tabs.Trigger>
        <Tabs.Trigger
          class="settings-nav-tab {activeTab === 'operational' ? 'active' : ''}"
          value="operational"
        >
          <SlidersHorizontal size={15} />
          <span>{tr('Operational settings')}</span>
        </Tabs.Trigger>
        <Tabs.Trigger
          class="settings-nav-tab {activeTab === 'resources' ? 'active' : ''}"
          value="resources"
        >
          <Cpu size={15} />
          <span>{tr('Gateway resource limits')}</span>
        </Tabs.Trigger>
        <Tabs.Trigger
          class="settings-nav-tab {activeTab === 'output-styles' ? 'active' : ''}"
          value="output-styles"
        >
          <Sparkles size={15} />
          <span>{tr('Output styles')}</span>
        </Tabs.Trigger>
        <Tabs.Trigger
          class="settings-nav-tab {activeTab === 'backup' ? 'active' : ''}"
          value="backup"
        >
          <Database size={15} />
          <span>{tr('Database backup')}</span>
        </Tabs.Trigger>
        <Tabs.Indicator class="settings-nav-indicator" />
      </Tabs.List>

      <!-- Tab 1: Server & Security -->
      <Tabs.Content class="settings-tab-pane {activeTab === 'server' ? 'active' : ''}" value="server">
      <!-- Quick Status Hero Banner -->
      <div class="settings-hero-banner">
        <div class="settings-hero-card">
          <div class="hero-card-icon status-live">
            <Radio size={18} />
          </div>
          <div class="hero-card-info">
            <span class="hero-card-label">
              <span class="pulse-dot"></span>
              {tr('LIVE')}
            </span>
            <strong class="hero-card-value">{tr('Gateway Operational')}</strong>
            <small class="hero-card-sub">/api/v1/admin</small>
          </div>
        </div>

        <div class="settings-hero-card">
          <div class="hero-card-icon">
            <Server size={18} />
          </div>
          <div class="hero-card-info">
            <span class="hero-card-label">{tr('Environment')}</span>
            <strong class="hero-card-value font-mono">{settings.host ?? 'localhost'}:{settings.port ?? 8686}</strong>
            <small class="hero-card-sub">{settings.database_path ? tr('LMDB') : 'Loopback'}</small>
          </div>
        </div>

        <div class="settings-hero-card">
          <div class="hero-card-icon">
            <Activity size={18} />
          </div>
          <div class="hero-card-info">
            <span class="hero-card-label">{tr('Uptime')}</span>
            <strong class="hero-card-value">{overview ? formatUptime(overview.uptime_seconds, tr) : '—'}</strong>
            <small class="hero-card-sub">{tr('Time since the gateway started')}</small>
          </div>
        </div>

        <div class="settings-hero-card">
          <div class="hero-card-icon">
            <SlidersHorizontal size={18} />
          </div>
          <div class="hero-card-info">
            <span class="hero-card-label">{tr('Dropped request logs')}</span>
            <strong class="hero-card-value font-mono">{(overview?.dropped_request_logs ?? 0).toLocaleString(getIntlLocale(locale))}</strong>
            <small class="hero-card-sub">{tr('Records lost because the bounded log queue was full or LMDB could not write')}</small>
          </div>
        </div>
      </div>

      <!-- Server & Security Dual Grid -->
      <div class="settings-grid-dual">
        <!-- Server Panel -->
        <section class="settings-panel">
          <div class="panel-heading">
            <span class="panel-icon"><Server size={17} /></span>
            <div>
              <h2>{tr('Server')}</h2>
              <p>{tr('Gateway process and runtime')}</p>
            </div>
          </div>
          <div class="settings-row">
            <div>
              <strong>{tr('API base path')}</strong>
              <span>{tr('Admin endpoints used by this dashboard')}</span>
            </div>
            <code class="settings-badge-code">/api/v1/admin</code>
          </div>
          <div class="settings-row">
            <div>
              <strong>{tr('Environment')}</strong>
              <span>{tr('Connection target for the local dashboard')}</span>
            </div>
            <span class="value-with-dot"><i></i>{settings.host ?? 'localhost'}:{settings.port ?? 8686}</span>
          </div>
          <div class="settings-row">
            <div>
              <strong>{tr('database_path')}</strong>
              <span>{settings.database_path ?? '—'}</span>
            </div>
            <code class="settings-badge-code">{tr('LMDB')}</code>
          </div>
          <div class="settings-row">
            <div>
              <strong>{tr('Uptime')}</strong>
              <span>{tr('Time since the gateway started')}</span>
            </div>
            <span class="settings-badge-uptime">{overview ? formatUptime(overview.uptime_seconds, tr) : '—'}</span>
          </div>
        </section>

        <!-- Security Panel -->
        <section class="settings-panel">
          <div class="panel-heading">
            <span class="panel-icon violet-panel"><ShieldCheck size={17} /></span>
            <div>
              <h2>{tr('Security')}</h2>
              <p>{tr('Local dashboard access')}</p>
            </div>
          </div>
          <div class="settings-row">
            <div>
              <strong>{tr('Admin password')}</strong>
              <span>{tr(settings.admin_password_configured ? 'Stored as a hash in the database; included in backups' : 'No password configured')}</span>
            </div>
            <button class="primary-button compact" onclick={() => passwordDialogOpen = true}>
              <KeyRound size={14} />{tr('Change password')}
            </button>
          </div>
          <div class="settings-row">
            <div>
              <strong>{tr('Provider credentials')}</strong>
              <span>{tr('Secret values are sent only to the gateway')}</span>
            </div>
            {#if settings.master_key_configured}
              <span class="secure-value"><ShieldCheck size={14} /> {tr('Encrypted at rest')}</span>
            {:else}
              <span class="secure-value warning-value">{tr('Master key required')}</span>
            {/if}
          </div>
          <div class="settings-row">
            <div>
              <strong>{tr('Gateway API keys')}</strong>
              <span>{tr('Clients must send a key to use the /v1 endpoints')}</span>
            </div>
            <button class="secondary-button compact" onclick={() => onNavigate('api-keys')}>
              <KeyRound size={14} />{tr('Manage client API keys')}
            </button>
          </div>
        </section>
      </div>

      <section class="settings-panel settings-wide">
        <div class="panel-heading">
          <span class="panel-icon"><RefreshCw size={17} /></span>
          <div>
            <h2>{tr('Software updates')}</h2>
            <p>{tr('Check for a newer ExoRoute release without downloading anything')}</p>
          </div>
        </div>
        <div class="settings-row">
          <div>
            {#if updateCheckLoading}
              <strong>{tr('Checking for updates…')}</strong>
              <span>{tr('The latest stable release is being checked.')}</span>
            {:else if updateCheckFailed || updateCheck?.status === 'unavailable'}
              <strong>{tr('Update check unavailable')}</strong>
              <span>{tr('Could not check for updates. Try again later or use Check now.')}</span>
            {:else if updateCheck?.update_available}
              <strong>{tr('Version {version} is available', { version: `v${updateCheck.latest_version}` })}</strong>
              <span>{tr('Current version: {version}', { version: `v${updateCheck.current_version}` })}</span>
            {:else if updateCheck}
              <strong>{tr('You are running the latest version')}</strong>
              <span>{tr('Current version: {version}', { version: `v${updateCheck.current_version}` })}</span>
            {:else}
              <strong>{tr('Update status is not available')}</strong>
              <span>{tr('Use Check now to check the latest stable release.')}</span>
            {/if}
          </div>
          <div class="settings-update-actions">
            {#if updateCheck?.update_available && updateCheck.release_url}
              <a class="secondary-button compact" href={updateCheck.release_url} target="_blank" rel="noreferrer">
                <ExternalLink size={13} />{tr('View release')}
              </a>
            {/if}
            <button class="primary-button compact" disabled={updateCheckLoading} onclick={() => void checkForUpdates()}>
              {#if updateCheckLoading}<span class="auth-bootstrap-spinner"></span>{:else}<RefreshCw size={13} />{/if}{tr('Check now')}
            </button>
          </div>
        </div>
      </section>

      <!-- Public Configuration Panel -->
      <section class="settings-panel settings-wide">
        <div class="panel-heading">
          <span class="panel-icon amber"><SlidersHorizontal size={17} /></span>
          <div>
            <h2>{tr('Public configuration')}</h2>
            <p>{tr('Values reported by the running gateway')}</p>
          </div>
        </div>
        {#if Object.keys(settings).length}
          <div class="public-config-grid">
            {#each Object.entries(settings) as [key, value]}
              <div class="config-tile">
                <span class="config-tile-label">{configKeyLabel(key)}</span>
                {#if typeof value === 'boolean'}
                  <span class="config-status-chip" class:chip-active={value}>
                    <span class="chip-dot"></span>
                    {tr(String(value))}
                  </span>
                {:else}
                  <code class="config-tile-code">{String(value ?? '—')}</code>
                {/if}
              </div>
            {/each}
          </div>
        {:else}
          <div class="settings-placeholder">
            <span>—</span>
            <div>
              <strong>{tr('No public settings available')}</strong>
              <small>{tr('The API returned an empty configuration set.')}</small>
            </div>
          </div>
        {/if}
      </section>

      <div class="settings-note">
        <CircleHelp size={16} />
        <span>{tr('The admin password is stored as a salted hash in the database and included in database backups.')}</span>
      </div>
      </Tabs.Content>

      <!-- Tab 2: Operational Settings -->
      <Tabs.Content class="settings-tab-pane {activeTab === 'operational' ? 'active' : ''}" value="operational">
        <OperationalSettingsPanel {tr} refreshToken={panelRefreshToken} />
      </Tabs.Content>

      <!-- Output Styles -->
      <Tabs.Content class="settings-tab-pane {activeTab === 'output-styles' ? 'active' : ''}" value="output-styles">
        <OutputStylesPanel {tr} refreshToken={panelRefreshToken} />
      </Tabs.Content>

      <!-- Tab 3: Resource Limits -->
      <Tabs.Content class="settings-tab-pane {activeTab === 'resources' ? 'active' : ''}" value="resources">
        <GatewayResourceLimitsPanel {tr} refreshToken={panelRefreshToken} />
      </Tabs.Content>

      <!-- Tab 4: Database Backup -->
      <Tabs.Content class="settings-tab-pane {activeTab === 'backup' ? 'active' : ''}" value="backup">
        <DatabaseBackupPanel {tr} onAuthenticationReset={onAuthenticationReset} onImported={databaseImported} />
      </Tabs.Content>
    </Tabs.Root>
  {/if}
</div>

<AdminPasswordChangeDialog open={passwordDialogOpen} {tr} onClose={() => passwordDialogOpen = false} onComplete={passwordUpdated} />
