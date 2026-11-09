<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import type { DashboardPage } from '../../lib/navigation';
  import { showSketchToast } from '../../lib/sketch';
  import type {
    GatewayResourceLimitValues,
    OperationalSettingsValues,
    PublicSettings,
  } from '../../lib/types';
  import '../../styles/sketch/pages/settings.css';

  interface Props {
    tr: (key: string) => string;
    onNavigate: (page: DashboardPage) => void;
  }
  let { tr, onNavigate }: Props = $props();

  let publicSettings = $state<PublicSettings>({});
  let op = $state<OperationalSettingsValues | null>(null);
  let opRevision = $state(0);
  let limits = $state<GatewayResourceLimitValues | null>(null);
  let savedLimits = $state<GatewayResourceLimitValues | null>(null);
  let loading = $state(true);
  let error = $state('');
  let dirty = $state(false);
  let saving = $state(false);

  let pwDialog = $state<HTMLDialogElement | null>(null);
  let pwCurrent = $state('');
  let pwNew = $state('');
  let exportDialog = $state<HTMLDialogElement | null>(null);
  let exportPassword = $state('');
  let resetDialog = $state<HTMLDialogElement | null>(null);
  let resetTyped = $state('');

  const num = (v: number | undefined) => (v ?? 0).toString();

  async function load() {
    loading = true;
    error = '';
    try {
      const [s, o, l] = await Promise.all([
        api.settings(),
        api.operationalSettings(),
        api.gatewayResourceLimits(),
      ]);
      publicSettings = s;
      op = structuredClone(o.settings);
      opRevision = o.revision;
      limits = structuredClone(l.active);
      savedLimits = structuredClone(l.saved);
      dirty = false;
    } catch (cause) {
      error = localizedError(cause, 'Could not load settings.', tr);
    } finally {
      loading = false;
    }
  }
  onMount(() => {
    void load();
  });

  function markDirty() {
    dirty = true;
  }

  async function save() {
    if (!op || !limits || saving) return;
    saving = true;
    try {
      if (JSON.stringify(limits) !== JSON.stringify(savedLimits)) {
        await api.updateGatewayResourceLimits(limits, savedLimits!, limits.provider_max_concurrency === 0);
      }
      const snapshot = await api.updateOperationalSettings(
        op,
        opRevision,
        op.gateway_max_in_flight === 0,
      );
      op = structuredClone(snapshot.settings);
      opRevision = snapshot.revision;
      const l = await api.gatewayResourceLimits();
      limits = structuredClone(l.active);
      savedLimits = structuredClone(l.saved);
      dirty = false;
      showSketchToast('Settings saved');
    } catch (cause) {
      showSketchToast(localizedError(cause, 'Could not save settings.', tr));
    } finally {
      saving = false;
    }
  }

  async function submitPassword(event: SubmitEvent) {
    event.preventDefault();
    try {
      await api.changeAdminPassword(pwCurrent, pwNew);
      pwDialog?.close();
      pwCurrent = pwNew = '';
      showSketchToast('Admin password changed');
    } catch (cause) {
      showSketchToast(localizedError(cause, 'Could not change password.', tr));
    }
  }

  async function submitExport() {
    if (!exportPassword) return;
    try {
      const auth = await api.reauthenticateAdmin(exportPassword, 'database_export');
      exportPassword = '';
      exportDialog?.close();
      const { blob, filename } = await api.exportDatabase(false, auth.step_up_token);
      const a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = filename;
      a.click();
      URL.revokeObjectURL(a.href);
      showSketchToast('Database backup downloaded');
    } catch (cause) {
      showSketchToast(localizedError(cause, 'Export failed.', tr));
    }
  }

  async function deleteLogs() {
    try {
      const result = await api.deleteRequestLogs();
      showSketchToast(`Deleted ${result.deleted_count} request logs`);
    } catch (cause) {
      showSketchToast(localizedError(cause, 'Could not delete request logs.', tr));
    }
  }

  async function factoryResetOp() {
    try {
      const snapshot = await api.resetOperationalSettings(opRevision);
      op = structuredClone(snapshot.settings);
      opRevision = snapshot.revision;
      resetDialog?.close();
      resetTyped = '';
      dirty = false;
      showSketchToast('Operational settings reset to defaults');
    } catch (cause) {
      showSketchToast(localizedError(cause, 'Reset failed.', tr));
    }
  }

  function jump(id: string) {
    document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  const host = $derived(String(publicSettings.host ?? '127.0.0.1'));
  const port = $derived(String(publicSettings.port ?? '—'));
  const dbPath = $derived(String(publicSettings.database_path ?? '~/.exoroute/exoroute.lmdb'));
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>Settings</h1>
    <p>Gateway-wide configuration. Changes apply after saving.</p>
  </div>
  <div class="heading-actions">
    {#if dirty}<span class="dirty-badge">Unsaved changes</span>{/if}
    <button class="action-button" type="button" onclick={() => { void load(); }}>
      <span class="button-icon"><svg class="icon"><use href="#i-refresh" /></svg></span>
      <span class="button-label">Reload</span>
    </button>
    <button class="action-button primary" type="button" onclick={save} disabled={!dirty || saving || !op}>
      <span class="button-icon"><svg class="icon"><use href="#i-check" /></svg></span>
      <span class="button-label">{saving ? 'Saving…' : 'Save changes'}</span>
    </button>
  </div>
</div>

<div class="page-surface">
  {#if error}
    <div class="empty-state">{error} <button class="mini-add" type="button" onclick={() => load()}>Retry</button></div>
  {:else if loading || !op || !limits}
    <div class="empty-state">Loading…</div>
  {:else}
    <section class="stats-grid" aria-label="Settings overview">
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-server" /></svg></span><span class="stat-number">{port}</span></div>
        <h2>Listen Port</h2>
        <div class="stat-trend">{host} — local only</div>
        <button class="details-link" type="button" onclick={() => jump('sec-server')}>Server <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-shield" /></svg></span><span class="stat-number">{publicSettings.admin_password_configured ? 'On' : '—'}</span></div>
        <h2>Auth Required</h2>
        <div class="stat-trend">Admin session enforced</div>
        <button class="details-link" type="button" onclick={() => jump('sec-auth')}>Security <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-layers" /></svg></span><span class="stat-number">{op.circuit_breaker_enabled ? 'CB' : '—'}</span></div>
        <h2>Circuit Breaker</h2>
        <div class="stat-trend">{op.circuit_breaker_enabled ? `threshold ${op.circuit_breaker_threshold}` : 'disabled'}</div>
        <button class="details-link" type="button" onclick={() => jump('sec-routing')}>Routing <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-database" /></svg></span><span class="stat-number">LMDB</span></div>
        <h2>Storage</h2>
        <div class="stat-trend">{dbPath.split(/[\\/]/).pop()}</div>
        <button class="details-link" type="button" onclick={() => jump('sec-data')}>Data &amp; backup <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
    </section>

    <div class="settings-grid">
      <section class="course-card surface settings-card" id="sec-server">
        <div class="card-title-row"><h2>Server</h2><span class="status complete">Running</span></div>
        <p class="card-subtitle">Bind address and request limits. Bind settings require a restart and are read-only here.</p>
        <div class="settings-fields">
          <label class="field">Host<input type="text" value={host} readonly class="readonly-field" /></label>
          <label class="field">Port<input type="text" value={port} readonly class="readonly-field" /></label>
          <label class="field">Max body size (MiB)
            <input type="number" min="1" max="512" value={num(limits.gateway_body_limit_mib)} onchange={(e) => { limits!.gateway_body_limit_mib = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">SSE frame limit (KiB)
            <input type="number" min="0" value={num(limits.sse_frame_limit_kib)} onchange={(e) => { limits!.sse_frame_limit_kib = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Provider max concurrency (0 = unlimited)
            <input type="number" min="0" max="32" value={num(limits.provider_max_concurrency)} onchange={(e) => { limits!.provider_max_concurrency = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field check-row">
            <input type="checkbox" checked={limits.stream_continuity_enabled} onchange={(e) => { limits!.stream_continuity_enabled = (e.currentTarget as HTMLInputElement).checked; markDirty(); }} />
            Stream continuity (transparent mid-stream retries)
          </label>
        </div>
      </section>

      <section class="course-card surface settings-card" id="sec-auth">
        <div class="card-title-row"><h2>Auth &amp; security</h2><span class="status complete">Secure</span></div>
        <p class="card-subtitle">Admin API throttling and password management.</p>
        <div class="settings-fields">
          <label class="field">Admin API rate limit (requests)
            <input type="number" min="1" value={num(op.admin_api_max_requests)} onchange={(e) => { op!.admin_api_max_requests = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Rate limit window (seconds)
            <input type="number" min="1" value={num(op.admin_api_window_seconds)} onchange={(e) => { op!.admin_api_window_seconds = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Gateway key capacity
            <input type="number" min="1" value={num(op.gateway_key_capacity)} onchange={(e) => { op!.gateway_key_capacity = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Gateway key refill (tokens / interval ms)
            <input type="text" value={`${op.gateway_key_refill_tokens} / ${op.gateway_key_refill_interval_ms}`} readonly class="readonly-field" />
          </label>
          <div class="provd-wide">
            <button class="page-action" type="button" onclick={() => pwDialog?.showModal()}><svg class="icon"><use href="#i-shield" /></svg> Change admin password</button>
          </div>
        </div>
      </section>

      <section class="course-card surface settings-card" id="sec-routing">
        <div class="card-title-row"><h2>Routing defaults</h2></div>
        <p class="card-subtitle">Timeouts, retries and circuit breaker applied gateway-wide.</p>
        <div class="settings-fields">
          <label class="field">Connect timeout (ms)
            <input type="number" min="0" value={num(op.connect_timeout_ms)} onchange={(e) => { op!.connect_timeout_ms = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Request timeout (ms)
            <input type="number" min="0" value={num(op.request_timeout_ms)} onchange={(e) => { op!.request_timeout_ms = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Stream idle timeout (ms)
            <input type="number" min="0" value={num(op.stream_idle_timeout_ms)} onchange={(e) => { op!.stream_idle_timeout_ms = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Max in-flight requests (0 = unlimited)
            <input type="number" min="0" value={num(op.gateway_max_in_flight)} onchange={(e) => { op!.gateway_max_in_flight = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Upstream response limit (MiB)
            <input type="number" min="0" value={num(op.upstream_response_limit_mib)} onchange={(e) => { op!.upstream_response_limit_mib = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field check-row">
            <input type="checkbox" checked={op.circuit_breaker_enabled} onchange={(e) => { op!.circuit_breaker_enabled = (e.currentTarget as HTMLInputElement).checked; markDirty(); }} />
            Circuit breaker enabled
          </label>
          <label class="field">Circuit breaker threshold
            <input type="number" min="1" value={num(op.circuit_breaker_threshold)} onchange={(e) => { op!.circuit_breaker_threshold = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Circuit breaker cooldown (s)
            <input type="number" min="0" value={num(op.circuit_breaker_cooldown_seconds)} onchange={(e) => { op!.circuit_breaker_cooldown_seconds = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Server retry max attempts
            <input type="number" min="0" value={num(op.upstream.server_retry_max_attempts)} onchange={(e) => { op!.upstream.server_retry_max_attempts = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
        </div>
      </section>

      <section class="course-card surface settings-card" id="sec-logging">
        <div class="card-title-row"><h2>Logging &amp; telemetry</h2></div>
        <p class="card-subtitle">Request log retention stored in LMDB.</p>
        <div class="settings-fields">
          <label class="field">Request log retention (days)
            <input type="number" min="1" value={num(op.request_log_retention_days)} onchange={(e) => { op!.request_log_retention_days = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <label class="field">Request log max rows
            <input type="number" min="1" value={num(op.request_log_max_rows)} onchange={(e) => { op!.request_log_max_rows = Number((e.currentTarget as HTMLInputElement).value) || 0; markDirty(); }} />
          </label>
          <div class="provd-wide">
            <button class="page-action danger" type="button" onclick={deleteLogs}>Delete all request logs</button>
          </div>
        </div>
      </section>

      <section class="course-card surface settings-card" id="sec-data">
        <div class="card-title-row"><h2>Data &amp; backup</h2></div>
        <p class="card-subtitle">LMDB environment stored locally. Exports require admin re-authentication.</p>
        <div class="settings-fields">
          <label class="field provd-wide">Environment path
            <input type="text" value={dbPath} readonly class="readonly-field" />
          </label>
          <div class="provd-wide data-actions">
            <button class="page-action" type="button" onclick={() => exportDialog?.showModal()}><svg class="icon"><use href="#i-download" /></svg> Export backup</button>
            <button class="page-action" type="button" onclick={() => onNavigate('requests')}><svg class="icon"><use href="#i-clipboard" /></svg> View request logs</button>
          </div>
        </div>
      </section>

      <section class="course-card surface settings-card settings-danger" id="sec-danger">
        <div class="card-title-row"><h2>Danger zone</h2><span class="status error">Irreversible</span></div>
        <p class="card-subtitle">Reset operational settings to defaults. Providers, combos and keys are not affected.</p>
        <div class="settings-fields">
          <label class="field provd-wide">Type <code>RESET</code> to confirm
            <input type="text" placeholder="RESET" autocomplete="off" bind:value={resetTyped} />
          </label>
          <div class="provd-wide">
            <button class="page-action danger" type="button" disabled={resetTyped.trim() !== 'RESET'} onclick={() => resetDialog?.showModal()}>Reset operational settings</button>
          </div>
        </div>
      </section>
    </div>
  {/if}
</div>

<!-- Change admin password -->
<dialog class="modal" bind:this={pwDialog} aria-labelledby="pw-title">
  <form class="modal-card" onsubmit={submitPassword}>
    <div class="modal-heading">
      <div><h2 id="pw-title">Change admin password</h2><p>Requires your current password.</p></div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="modal-fields">
      <label class="field provd-wide">Current password<input type="password" required autocomplete="current-password" bind:value={pwCurrent} /></label>
      <label class="field provd-wide">New password<input type="password" required autocomplete="new-password" bind:value={pwNew} /></label>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Cancel</button>
      <button class="page-action primary" type="submit">Change password</button>
    </div>
  </form>
</dialog>

<!-- Export backup -->
<dialog class="modal" bind:this={exportDialog} aria-labelledby="export-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div><h2 id="export-title">Export database backup</h2><p>Enter your admin password to authorize one export.</p></div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="modal-fields">
      <label class="field provd-wide">Admin password<input type="password" required autocomplete="current-password" bind:value={exportPassword} /></label>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Cancel</button>
      <button class="page-action primary" type="button" onclick={submitExport} disabled={!exportPassword}>Authorize &amp; export</button>
    </div>
  </div>
</dialog>

<!-- Reset operational settings -->
<dialog class="modal" bind:this={resetDialog} aria-labelledby="reset-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div><h2 id="reset-title">Reset operational settings?</h2><p>All operational values return to their defaults. This takes effect immediately.</p></div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Cancel</button>
      <button class="page-action danger" type="button" onclick={factoryResetOp}>Reset to defaults</button>
    </div>
  </div>
</dialog>
