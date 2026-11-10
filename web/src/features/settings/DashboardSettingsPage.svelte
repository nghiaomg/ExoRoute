<script lang="ts">
  import { onMount } from 'svelte';
  import { Cpu, Database, Palette, Server, SlidersHorizontal, Sparkles } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import type { DashboardPage } from '../../lib/navigation';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Translate } from '../../lib/format';
  import type { ThemeId, ThemePreference } from '../../lib/theme';
  import type { PublicSettings } from '../../lib/types';
  import AppearancePanel from './AppearancePanel.svelte';
  import DatabaseBackupPanel from './DatabaseBackupPanel.svelte';
  import GatewayResourceLimitsPanel from './GatewayResourceLimitsPanel.svelte';
  import OperationalSettingsPanel from './OperationalSettingsPanel.svelte';
  import OutputStylesPanel from './OutputStylesPanel.svelte';
  import '../../styles/dashboard/pages/settings.css';

  interface Props {
    tr: Translate;
    theme: ThemeId;
    themePreference: ThemePreference;
    onThemeChange: (id: ThemeId) => void;
    onThemePreferenceChange: (preference: ThemePreference) => void;
    onNavigate: (page: DashboardPage) => void;
    onAuthenticationReset: () => void;
  }
  let { tr, theme, themePreference, onThemeChange, onThemePreferenceChange, onNavigate, onAuthenticationReset }: Props = $props();

  type SettingsTab = 'server' | 'operational' | 'resources' | 'output-styles' | 'appearance' | 'backup';

  const tabs: Array<{ id: SettingsTab; label: string; icon: typeof Server }> = [
    { id: 'server', label: 'Server & Security', icon: Server },
    { id: 'operational', label: 'Operational settings', icon: SlidersHorizontal },
    { id: 'resources', label: 'Gateway resource limits', icon: Cpu },
    { id: 'output-styles', label: 'Output styles', icon: Sparkles },
    { id: 'appearance', label: 'Appearance', icon: Palette },
    { id: 'backup', label: 'Database backup', icon: Database },
  ];

  let activeTab: SettingsTab = $state('server');
  let settings = $state<PublicSettings>({});
  let loading = $state(true);
  let error = $state('');
  // Bumped after a database import so mounted panels reload values that may
  // have been replaced by the restored backup.
  let panelRefreshToken = $state(0);

  let pwDialog = $state<HTMLDialogElement | null>(null);
  let pwCurrent = $state('');
  let pwNew = $state('');

  async function loadPublicSettings(): Promise<void> {
    loading = true;
    error = '';
    try {
      settings = await api.settings();
    } catch (cause) {
      error = localizedError(cause, 'Could not load settings.', tr);
    } finally {
      loading = false;
    }
  }

  function selectTab(tab: SettingsTab): void {
    activeTab = tab;
    try {
      window.history.replaceState(null, '', `#${tab}`);
    } catch {
      // Ignore in sandboxed contexts
    }
  }

  function databaseImported(): void {
    panelRefreshToken += 1;
    void loadPublicSettings();
  }

  async function submitPassword(event: SubmitEvent) {
    event.preventDefault();
    try {
      await api.changeAdminPassword(pwCurrent, pwNew);
      pwDialog?.close();
      pwCurrent = pwNew = '';
      showDashboardToast(tr('Admin password changed'));
    } catch (cause) {
      showDashboardToast(localizedError(cause, 'Could not change password.', tr));
    }
  }

  async function deleteLogs() {
    try {
      const result = await api.deleteRequestLogs();
      showDashboardToast(tr('Deleted {count} request logs', { count: result.deleted_count }));
    } catch (cause) {
      showDashboardToast(localizedError(cause, 'Could not delete request logs.', tr));
    }
  }

  onMount(() => {
    try {
      const hash = window.location.hash.replace('#', '');
      if (tabs.some((tab) => tab.id === hash)) activeTab = hash as SettingsTab;
    } catch {
      // Ignore
    }
    void loadPublicSettings();
  });

  const host = $derived(String(settings.host ?? '127.0.0.1'));
  const port = $derived(String(settings.port ?? '—'));
  const dbPath = $derived(String(settings.database_path ?? '~/.exoroute/exoroute.lmdb'));
</script>

<div class="settings-view">
  <div class="page-heading">
    <div class="heading-copy">
      <h1>{tr('Settings')}</h1>
      <p>{tr('Runtime information and public gateway configuration.')}</p>
    </div>
  </div>

  <div class="settings-nav-tabs" role="tablist" aria-label={tr('Settings')}>
    {#each tabs as tab (tab.id)}
      <button
        class="settings-nav-tab"
        class:active={activeTab === tab.id}
        data-selected={activeTab === tab.id ? '' : undefined}
        type="button"
        role="tab"
        aria-selected={activeTab === tab.id}
        onclick={() => selectTab(tab.id)}
      >
        <tab.icon size={15} />
        <span>{tr(tab.label)}</span>
      </button>
    {/each}
  </div>

  {#if error}
    <section class="course-card surface settings-card" role="alert">
      <div class="card-title-row"><h2>{error}</h2></div>
      <button class="page-action" type="button" onclick={() => void loadPublicSettings()}>{tr('Retry')}</button>
    </section>
  {:else if loading}
    <section class="course-card surface settings-card"><div class="empty-state">{tr('Loading…')}</div></section>
  {:else}
    {#if activeTab === 'server'}
      <section class="stats-grid" aria-label={tr('Settings overview')}>
        <article class="stat-card surface">
          <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-server" /></svg></span><span class="stat-number">{port}</span></div>
          <h2>{tr('Listen port')}</h2>
          <div class="stat-trend">{host}</div>
          <button class="details-link" type="button" onclick={() => selectTab('operational')}>{tr('Operational settings')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
        </article>
        <article class="stat-card surface">
          <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-shield" /></svg></span><span class="stat-number">{settings.admin_password_configured ? tr('On') : '—'}</span></div>
          <h2>{tr('Auth Required')}</h2>
          <div class="stat-trend">{tr('Admin session enforced')}</div>
          <button class="details-link" type="button" onclick={() => pwDialog?.showModal()}>{tr('Change admin password')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
        </article>
        <article class="stat-card surface">
          <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-layers" /></svg></span><span class="stat-number">{tr('Resource limits')}</span></div>
          <h2>{tr('Gateway resource limits')}</h2>
          <div class="stat-trend">{tr('Body size, SSE buffers and provider concurrency')}</div>
          <button class="details-link" type="button" onclick={() => selectTab('resources')}>{tr('Gateway resource limits')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
        </article>
        <article class="stat-card surface">
          <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-database" /></svg></span><span class="stat-number">LMDB</span></div>
          <h2>{tr('Storage')}</h2>
          <div class="stat-trend">{dbPath.split(/[\\/]/).pop()}</div>
          <button class="details-link" type="button" onclick={() => selectTab('backup')}>{tr('Database backup')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
        </article>
      </section>

      <div class="settings-grid">
        <section class="course-card surface settings-card" id="sec-server">
          <div class="card-title-row"><h2>{tr('Server')}</h2><span class="status complete">{tr('Running')}</span></div>
          <p class="card-subtitle">{tr('Bind address and request limits. Bind settings require a restart and are read-only here.')}</p>
          <div class="settings-fields">
            <label class="field">{tr('Host')}<input type="text" value={host} readonly class="readonly-field" /></label>
            <label class="field">{tr('Port')}<input type="text" value={port} readonly class="readonly-field" /></label>
            <label class="field provd-wide">{tr('database_path')}<input type="text" value={dbPath} readonly class="readonly-field" /></label>
          </div>
        </section>

        <section class="course-card surface settings-card" id="sec-auth">
          <div class="card-title-row"><h2>{tr('Auth & security')}</h2><span class="status complete">{tr('Secure')}</span></div>
          <p class="card-subtitle">{tr('Admin API throttling and password management.')}</p>
          <div class="settings-fields">
            <div class="provd-wide">
              <button class="page-action" type="button" onclick={() => pwDialog?.showModal()}><svg class="icon"><use href="#i-shield" /></svg> {tr('Change admin password')}</button>
            </div>
            <div class="provd-wide">
              <button class="page-action" type="button" onclick={() => selectTab('operational')}><svg class="icon"><use href="#i-arrow-right" /></svg> {tr('Operational settings')}</button>
            </div>
          </div>
        </section>

        <section class="course-card surface settings-card" id="sec-logging">
          <div class="card-title-row"><h2>{tr('Logging & telemetry')}</h2></div>
          <p class="card-subtitle">{tr('Request log retention stored in LMDB.')}</p>
          <div class="settings-fields">
            <div class="provd-wide">
              <button class="page-action" type="button" onclick={() => selectTab('operational')}><svg class="icon"><use href="#i-arrow-right" /></svg> {tr('Operational settings')}</button>
              <button class="page-action" type="button" onclick={() => onNavigate('requests')}><svg class="icon"><use href="#i-clipboard" /></svg> {tr('View request logs')}</button>
            </div>
            <div class="provd-wide">
              <button class="page-action danger" type="button" onclick={deleteLogs}>{tr('Delete all request logs')}</button>
            </div>
          </div>
        </section>
      </div>
    {:else if activeTab === 'operational'}
      <OperationalSettingsPanel tr={tr} refreshToken={panelRefreshToken} />
    {:else if activeTab === 'resources'}
      <GatewayResourceLimitsPanel tr={tr} refreshToken={panelRefreshToken} />
    {:else if activeTab === 'output-styles'}
      <OutputStylesPanel tr={tr} refreshToken={panelRefreshToken} />
    {:else if activeTab === 'appearance'}
      <AppearancePanel {tr} {theme} {themePreference} onThemeChange={onThemeChange} onThemePreferenceChange={onThemePreferenceChange} />
    {:else if activeTab === 'backup'}
      <DatabaseBackupPanel {tr} onAuthenticationReset={onAuthenticationReset} onImported={databaseImported} />
    {/if}
  {/if}
</div>

<!-- Change admin password -->
<dialog class="modal" bind:this={pwDialog} aria-labelledby="pw-title">
  <form class="modal-card" onsubmit={submitPassword}>
    <div class="modal-heading">
      <div><h2 id="pw-title">{tr('Change admin password')}</h2><p>{tr('Requires your current password.')}</p></div>
      <button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</button>
    </div>
    <div class="modal-fields">
      <label class="field provd-wide">{tr('Current password')}<input type="password" required autocomplete="current-password" bind:value={pwCurrent} /></label>
      <label class="field provd-wide">{tr('New password')}<input type="password" required autocomplete="new-password" bind:value={pwNew} /></label>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>{tr('Cancel')}</button>
      <button class="page-action primary" type="submit">{tr('Change password')}</button>
    </div>
  </form>
</dialog>
