<script lang="ts">
  import Button from "../../components/Button.svelte";
  import StatsCard from "../../components/StatsCard.svelte";
  import { onMount } from 'svelte';
  import { Cpu, Database, Palette, Server, SlidersHorizontal, Sparkles } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import Input from '../../components/Input.svelte';
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
      <Button
        class="settings-nav-tab" stateClasses={{ active: activeTab === tab.id }}

        data-selected={activeTab === tab.id ? '' : undefined}
        type="button"
        role="tab"
        aria-selected={activeTab === tab.id}
        onclick={() => selectTab(tab.id)}
      >
        <tab.icon size={14} />
        <span>{tr(tab.label)}</span>
      </Button>
    {/each}
  </div>

  {#if error}
    <section class="course-card surface flex flex-col" role="alert">
      <div class="card-title-row"><h2>{error}</h2></div>
      <Button class="page-action" type="button" onclick={() => void loadPublicSettings()}>{tr('Retry')}</Button>
    </section>
  {:else if loading}
    <section class="course-card surface flex flex-col"><div class="empty-state">{tr('Loading…')}</div></section>
  {:else}
    {#if activeTab === 'server'}
      <section class="stats-grid" aria-label={tr('Settings overview')}>
        <StatsCard index={1} eyebrow={tr('Config')} value={port} title={tr('Listen port')} trend={host} />
        <StatsCard index={2} eyebrow={tr('Security')} badgeTone="success" badge={settings.admin_password_configured ? tr('On') : undefined} value={settings.admin_password_configured ? tr('On') : '—'} title={tr('Auth Required')} trend={tr('Admin session enforced')} />
        <StatsCard index={3} eyebrow={tr('Limits')} value={tr('Resource limits')} title={tr('Gateway resource limits')} trend={tr('Body size, SSE buffers and provider concurrency')} />
        <StatsCard index={4} eyebrow={tr('Storage')} value="LMDB" title={tr('Storage')} trend={dbPath.split(/[\\/]/).pop()} />
      </section>

      <div class="grid grid-cols-2 gap-[18px] max-[1100px]:grid-cols-1">
        <section class="course-card surface flex flex-col" id="sec-server">
          <div class="card-title-row"><h2>{tr('Server')}</h2><span class="status complete">{tr('Running')}</span></div>
          <p class="card-subtitle">{tr('Bind address and request limits. Bind settings require a restart and are read-only here.')}</p>
          <div class="mt-3.5 grid grid-cols-2 gap-x-4 gap-y-3.5 max-nav:grid-cols-1">
            <label class="field">{tr('Host')}<Input type="text" value={host} readonly class="bg-surface-sunken! text-muted!" /></label>
            <label class="field">{tr('Port')}<Input type="text" value={port} readonly class="bg-surface-sunken! text-muted!" /></label>
            <label class="field col-span-full">{tr('database_path')}<Input type="text" value={dbPath} readonly class="bg-surface-sunken! text-muted!" /></label>
          </div>
        </section>

        <section class="course-card surface flex flex-col" id="sec-auth">
          <div class="card-title-row"><h2>{tr('Auth & security')}</h2><span class="status complete">{tr('Secure')}</span></div>
          <p class="card-subtitle">{tr('Admin API throttling and password management.')}</p>
          <div class="mt-3.5 grid grid-cols-2 gap-x-4 gap-y-3.5 max-nav:grid-cols-1">
            <div class="col-span-full">
              <Button class="page-action" type="button" onclick={() => pwDialog?.showModal()}><svg class="icon"><use href="#i-shield" /></svg> {tr('Change admin password')}</Button>
            </div>
            <div class="col-span-full">
              <Button class="page-action" type="button" onclick={() => selectTab('operational')}><svg class="icon"><use href="#i-arrow-right" /></svg> {tr('Operational settings')}</Button>
            </div>
          </div>
        </section>

        <section class="course-card surface flex flex-col" id="sec-logging">
          <div class="card-title-row"><h2>{tr('Logging & telemetry')}</h2></div>
          <p class="card-subtitle">{tr('Request log retention stored in LMDB.')}</p>
          <div class="mt-3.5 grid grid-cols-2 gap-x-4 gap-y-3.5 max-nav:grid-cols-1">
            <div class="col-span-full">
              <Button class="page-action" type="button" onclick={() => selectTab('operational')}><svg class="icon"><use href="#i-arrow-right" /></svg> {tr('Operational settings')}</Button>
              <Button class="page-action" type="button" onclick={() => onNavigate('requests')}><svg class="icon"><use href="#i-clipboard" /></svg> {tr('View request logs')}</Button>
            </div>
            <div class="col-span-full">
              <Button class="page-action danger" type="button" onclick={deleteLogs}>{tr('Delete all request logs')}</Button>
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
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="modal-fields">
      <label class="field col-span-full">{tr('Current password')}<Input type="password" required autocomplete="current-password" bind:value={pwCurrent} /></label>
      <label class="field col-span-full">{tr('New password')}<Input type="password" required autocomplete="new-password" bind:value={pwNew} /></label>
    </div>
    <div class="modal-actions">
      <Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button>
      <Button class="page-action primary" type="submit">{tr('Change password')}</Button>
    </div>
  </form>
</dialog>
