<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { CircleHelp, LoaderCircle, RotateCcw, Save, SlidersHorizontal } from '@lucide/svelte';
  import ArkCheckbox from '../../components/ArkCheckbox.svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import { ApiError, api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { OperationalSettingsSnapshot, OperationalSettingsValues } from '../../lib/types';
  import ConfigHelpDialog from './ConfigHelpDialog.svelte';
  import { coreHelpMap, defaults, upstreamFields, type ConfigHelp, type UpstreamKey } from './operational-settings.config';

  export let tr: Translate;
  export let refreshToken = 0;


  let snapshot: OperationalSettingsSnapshot | null = null;
  let draft: OperationalSettingsValues = { ...defaults };
  let busy: '' | 'loading' | 'saving' | 'resetting' = 'loading';
  let errorMessage = '';
  let successMessage = '';
  let reloadConflict = false;
  let unlimitedWarningOpen = false;
  let activeHelp: ConfigHelp | null = null;
  let help: ConfigHelp | null = null;
  let active = true;
  let requestGeneration = 0;
  let observedRefreshToken = 0;

  function openHelp(help: ConfigHelp): void {
    help = help;
  }

  $: draftIsValid = Number.isSafeInteger(draft.connect_timeout_ms)
    && draft.connect_timeout_ms >= 100 && draft.connect_timeout_ms <= 120_000
    && Number.isSafeInteger(draft.request_timeout_ms)
    && draft.request_timeout_ms >= 100 && draft.request_timeout_ms <= 86_400_000
    && Number.isSafeInteger(draft.stream_idle_timeout_ms)
    && draft.stream_idle_timeout_ms >= 100 && draft.stream_idle_timeout_ms <= 3_600_000
    && Number.isSafeInteger(draft.circuit_breaker_threshold)
    && draft.circuit_breaker_threshold >= 1 && draft.circuit_breaker_threshold <= 100
    && Number.isSafeInteger(draft.circuit_breaker_cooldown_seconds)
    && draft.circuit_breaker_cooldown_seconds >= 1 && draft.circuit_breaker_cooldown_seconds <= 86_400
    && Number.isSafeInteger(draft.gateway_max_in_flight)
    && (draft.gateway_max_in_flight === 0 || (draft.gateway_max_in_flight >= 1 && draft.gateway_max_in_flight <= 64))
    && Number.isSafeInteger(draft.upstream_response_limit_mib)
    && draft.upstream_response_limit_mib >= 1 && draft.upstream_response_limit_mib <= 16
    && Number.isSafeInteger(draft.admin_api_max_requests)
    && draft.admin_api_max_requests >= 1 && draft.admin_api_max_requests <= 4_294_967_295
    && Number.isSafeInteger(draft.admin_api_window_seconds)
    && draft.admin_api_window_seconds >= 1 && draft.admin_api_window_seconds <= 86_400
    && Number.isSafeInteger(draft.gateway_key_capacity)
    && draft.gateway_key_capacity >= 1 && draft.gateway_key_capacity <= 4_294_967_295
    && Number.isSafeInteger(draft.gateway_key_refill_tokens)
    && draft.gateway_key_refill_tokens >= 1 && draft.gateway_key_refill_tokens <= 4_294_967_295
    && Number.isSafeInteger(draft.gateway_key_refill_interval_ms)
    && draft.gateway_key_refill_interval_ms >= 1 && draft.gateway_key_refill_interval_ms <= 86_400_000
    && Number.isSafeInteger(draft.request_log_retention_days)
    && draft.request_log_retention_days >= 1 && draft.request_log_retention_days <= 365
    && Number.isSafeInteger(draft.request_log_max_rows)
    && draft.request_log_max_rows >= 1 && draft.request_log_max_rows <= 100_000
    && upstreamIsValid;
  $: upstreamIsValid = upstreamFields.every((field) => {
    const value = draft.upstream[field.key];
    return Number.isSafeInteger(value) && value >= field.min && value <= field.max;
  })
    && draft.upstream.continuity_replay_bytes_total_mib >= draft.upstream.continuity_replay_bytes_per_run_mib
    && draft.upstream.continuity_retry_max_delay_ms >= draft.upstream.continuity_retry_base_delay_ms
    && draft.upstream.provider_live_events_keepalive_seconds < draft.upstream.provider_live_events_max_duration_seconds
    && draft.upstream.command_code_optional_usage_timeout_seconds <= draft.upstream.command_code_usage_timeout_seconds
    && draft.upstream.remote_image_total_max_mib >= draft.upstream.remote_image_max_mib;
  $: hasChanges = snapshot !== null
    && (!snapshot.overridden || !sameSettings(draft, snapshot.settings));
  $: if (refreshToken > observedRefreshToken) {
    observedRefreshToken = refreshToken;
    void load();
  }

  function sameSettings(left: OperationalSettingsValues, right: OperationalSettingsValues): boolean {
    const topLevelKeys = (Object.keys(defaults) as (keyof OperationalSettingsValues)[])
      .filter((key) => key !== 'upstream');
    return topLevelKeys.every((key) => left[key] === right[key])
      && upstreamFields.every(({ key }) => left.upstream[key] === right.upstream[key]);
  }

  function updateUpstream(key: UpstreamKey, event: Event): void {
    const value = Number((event.currentTarget as HTMLInputElement).value);
    draft = { ...draft, upstream: { ...draft.upstream, [key]: value } };
  }

  function applySnapshot(result: OperationalSettingsSnapshot): void {
    snapshot = result;
    draft = { ...result.settings };
  }

  async function load(): Promise<void> {
    const currentRequest = ++requestGeneration;
    busy = 'loading';
    errorMessage = '';
    successMessage = '';
    reloadConflict = false;
    unlimitedWarningOpen = false;
    try {
      const result = await api.operationalSettings();
      if (!active || currentRequest !== requestGeneration) return;
      applySnapshot(result);
    } catch (error) {
      if (!active || currentRequest !== requestGeneration) return;
      errorMessage = localizedError(error, 'Could not load operational settings.', tr);
    } finally {
      if (active && currentRequest === requestGeneration) busy = '';
    }
  }

  async function reloadLatest(): Promise<void> {
    busy = 'loading';
    errorMessage = '';
    try {
      const result = await api.operationalSettings();
      if (!active) return;
      applySnapshot(result);
      reloadConflict = false;
      successMessage = tr('Latest operational settings loaded.');
    } catch (error) {
      if (active) errorMessage = localizedError(error, 'Could not load operational settings.', tr);
    } finally {
      if (active) busy = '';
    }
  }

  function save(event: SubmitEvent): void {
    event.preventDefault();
    if (busy || !draftIsValid || !hasChanges || !snapshot) return;
    if (draft.gateway_max_in_flight === 0) {
      unlimitedWarningOpen = true;
      return;
    }
    void applyDraft(false);
  }

  async function confirmUnlimited(): Promise<void> {
    unlimitedWarningOpen = false;
    await applyDraft(true);
  }

  async function applyDraft(confirmUnlimited: boolean): Promise<void> {
    if (busy || !snapshot || !draftIsValid || !hasChanges) return;
    const values = { ...draft };
    const expectedRevision = snapshot.revision;
    busy = 'saving';
    errorMessage = '';
    successMessage = '';
    reloadConflict = false;
    try {
      const result = await api.updateOperationalSettings(values, expectedRevision, confirmUnlimited);
      if (!active) return;
      applySnapshot(result);
      successMessage = tr('Operational settings saved and applied immediately.');
    } catch (error) {
      if (!active) return;
      if (error instanceof ApiError && error.status === 0) {
        try {
          const latest = await api.operationalSettings();
          if (!active) return;
          snapshot = latest;
          if (latest.overridden && sameSettings(latest.settings, values)) {
            draft = { ...latest.settings };
            successMessage = tr('Settings were saved; the response was interrupted.');
          } else {
            reloadConflict = latest.revision !== expectedRevision;
            errorMessage = localizedError(error, 'Could not save operational settings.', tr);
          }
        } catch {
          errorMessage = localizedError(error, 'Could not save operational settings.', tr);
        }
      } else {
        errorMessage = localizedError(error, 'Could not save operational settings.', tr);
        reloadConflict = error instanceof ApiError && error.status === 409;
        if (reloadConflict) {
          try {
            snapshot = await api.operationalSettings();
          } catch {
            // Keep the draft and conflict state so the user can retry loading.
          }
        }
      }
    } finally {
      if (active) busy = '';
    }
  }

  async function resetToEnvironment(): Promise<void> {
    if (busy || !snapshot) return;
    const expectedRevision = snapshot.revision;
    busy = 'resetting';
    errorMessage = '';
    successMessage = '';
    reloadConflict = false;
    try {
      const result = await api.resetOperationalSettings(expectedRevision);
      if (!active) return;
      applySnapshot(result);
      successMessage = tr('Operational settings reset to the startup environment and applied immediately.');
    } catch (error) {
      if (!active) return;
      if (error instanceof ApiError && error.status === 0) {
        try {
          const latest = await api.operationalSettings();
          if (!active) return;
          snapshot = latest;
          if (latest.revision > expectedRevision && !latest.overridden) {
            draft = { ...latest.settings };
            successMessage = tr('Settings reset successfully; the response was interrupted.');
          } else {
            reloadConflict = latest.revision !== expectedRevision;
            errorMessage = localizedError(error, 'Could not reset operational settings.', tr);
          }
        } catch {
          errorMessage = localizedError(error, 'Could not reset operational settings.', tr);
        }
      } else {
        errorMessage = localizedError(error, 'Could not reset operational settings.', tr);
        reloadConflict = error instanceof ApiError && error.status === 409;
      }
    } finally {
      if (active) busy = '';
    }
  }

  onMount(() => { void load(); });
  onDestroy(() => {
    active = false;
    requestGeneration += 1;
  });
</script>

<section class="settings-panel settings-wide resource-limits-panel">
  <div class="panel-heading">
    <span class="panel-icon amber"><SlidersHorizontal size={17} /></span>
    <div>
      <h2>{tr('Operational settings')}</h2>
      <p>{tr('Configure gateway timeouts, admission, authenticated quotas, and request-log retention.')}</p>
    </div>
  </div>

  {#if busy === 'loading' && snapshot === null}
    <div class="resource-limits-state" aria-live="polite"><LoaderCircle size={15} class="spin" />{tr('Loading operational settings…')}</div>
  {:else if errorMessage && snapshot === null}
    <div class="resource-limits-state resource-limits-error" role="alert">
      <span>{errorMessage}</span>
      <button class="secondary-button compact" type="button" onclick={load}>{tr('Retry')}</button>
    </div>
  {:else if snapshot}
    <div class="resource-limits-content">
      <div class="resource-limits-active">
        <strong>{tr('Active source')}</strong>
        <span>{tr(snapshot.overridden ? 'Database override' : 'Environment/default settings')} · {tr('Revision {revision}', { revision: snapshot.revision })}</span>
      </div>

      {#if errorMessage}
        <div class="resource-limits-error" role="alert">
          <span>{errorMessage}</span>
          {#if reloadConflict}<button class="secondary-button compact" type="button" onclick={reloadLatest}>{tr('Reload latest settings')}</button>{/if}
        </div>
      {/if}
      {#if successMessage}<div class="resource-limits-success" role="status">{successMessage}</div>{/if}

      <form class="resource-limits-form" onsubmit={save}>
        <div class="resource-limits-grid">
          <label class="resource-limit-field" for="operational-connect-timeout">
            <div class="field-title-row">
              <span>{tr('Upstream connect timeout')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.connect_timeout_ms);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-connect-timeout" type="number" min="100" max="120000" step="1" bind:value={draft.connect_timeout_ms} disabled={busy !== ''} /><small>ms</small></span>
            <small>{tr('Allowed range: 100–120000 ms. Default: 10000 ms.')}</small>
          </label>
          <label class="resource-limit-field" for="operational-request-timeout">
            <div class="field-title-row">
              <span>{tr('Upstream request timeout')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.request_timeout_ms);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-request-timeout" type="number" min="100" max="86400000" step="1" bind:value={draft.request_timeout_ms} disabled={busy !== ''} /><small>ms</small></span>
            <small>{tr('Allowed range: 100 ms–24 hours. Default: 300000 ms.')}</small>
          </label>
          <label class="resource-limit-field" for="operational-stream-timeout">
            <div class="field-title-row">
              <span>{tr('Stream idle timeout')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.stream_idle_timeout_ms);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-stream-timeout" type="number" min="100" max="3600000" step="1" bind:value={draft.stream_idle_timeout_ms} disabled={busy !== ''} /><small>ms</small></span>
            <small>{tr('Allowed range: 100 ms–1 hour. Default: 120000 ms.')}</small>
          </label>
          <div class="resource-limit-field">
            <div class="field-title-row">
              <span>{tr('Circuit breaker')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.circuit_breaker_enabled);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <ArkCheckbox checked={draft.circuit_breaker_enabled} disabled={busy !== ''} label={tr(draft.circuit_breaker_enabled ? 'Enabled' : 'Disabled')} onCheckedChange={(checked) => draft = { ...draft, circuit_breaker_enabled: checked === true }} />
            <small>{tr('Disabling it bypasses open circuits while preserving their failure state.')}</small>
          </div>
          <label class="resource-limit-field" for="operational-circuit-threshold">
            <div class="field-title-row">
              <span>{tr('Circuit breaker failure threshold')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.circuit_breaker_threshold);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-circuit-threshold" type="number" min="1" max="100" step="1" bind:value={draft.circuit_breaker_threshold} disabled={busy !== ''} /><small>{tr('failures')}</small></span>
          </label>
          <label class="resource-limit-field" for="operational-circuit-cooldown">
            <div class="field-title-row">
              <span>{tr('Circuit breaker cooldown')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.circuit_breaker_cooldown_seconds);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-circuit-cooldown" type="number" min="1" max="86400" step="1" bind:value={draft.circuit_breaker_cooldown_seconds} disabled={busy !== ''} /><small>{tr('seconds')}</small></span>
          </label>
          <label class="resource-limit-field" for="operational-gateway-in-flight">
            <div class="field-title-row">
              <span>{tr('Total concurrent gateway requests')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.gateway_max_in_flight);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-gateway-in-flight" type="number" min="0" max="64" step="1" bind:value={draft.gateway_max_in_flight} disabled={busy !== ''} /><small>{tr('requests')}</small></span>
            <small>{tr('Choose 1–64, or 0 for unlimited. Default: 64.')}</small>
          </label>
          <label class="resource-limit-field" for="operational-response-limit">
            <div class="field-title-row">
              <span>{tr('Upstream response limit')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.upstream_response_limit_mib);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-response-limit" type="number" min="1" max="16" step="1" bind:value={draft.upstream_response_limit_mib} disabled={busy !== ''} /><small>MiB</small></span>
            <small>{tr('Allowed range: 1–16 MiB. The hard ceiling remains 16 MiB.')}</small>
          </label>
          <label class="resource-limit-field" for="operational-admin-api-limit">
            <div class="field-title-row">
              <span>{tr('Authenticated admin API requests')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.admin_api_max_requests);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-admin-api-limit" type="number" min="1" max="4294967295" step="1" bind:value={draft.admin_api_max_requests} disabled={busy !== ''} /><small>{tr('requests')}</small></span>
          </label>
          <label class="resource-limit-field" for="operational-admin-api-window">
            <div class="field-title-row">
              <span>{tr('Authenticated admin API window')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.admin_api_window_seconds);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-admin-api-window" type="number" min="1" max="86400" step="1" bind:value={draft.admin_api_window_seconds} disabled={busy !== ''} /><small>{tr('seconds')}</small></span>
            <small>{tr('Login and authentication-failure throttles remain fixed.')}</small>
          </label>
          <label class="resource-limit-field" for="operational-gateway-burst">
            <div class="field-title-row">
              <span>{tr('Gateway API key token capacity')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.gateway_key_capacity);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-gateway-burst" type="number" min="1" max="4294967295" step="1" bind:value={draft.gateway_key_capacity} disabled={busy !== ''} /><small>{tr('tokens')}</small></span>
          </label>
          <label class="resource-limit-field" for="operational-gateway-refill-tokens">
            <div class="field-title-row">
              <span>{tr('Gateway API key refill amount')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.gateway_key_refill_tokens);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-gateway-refill-tokens" type="number" min="1" max="4294967295" step="1" bind:value={draft.gateway_key_refill_tokens} disabled={busy !== ''} /><small>{tr('tokens')}</small></span>
          </label>
          <label class="resource-limit-field" for="operational-gateway-refill-interval">
            <div class="field-title-row">
              <span>{tr('Gateway API key refill interval')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.gateway_key_refill_interval_ms);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-gateway-refill-interval" type="number" min="1" max="86400000" step="1" bind:value={draft.gateway_key_refill_interval_ms} disabled={busy !== ''} /><small>ms</small></span>
            <small>{tr('Refill amount and interval define the token-bucket refill rate.')}</small>
          </label>
          <label class="resource-limit-field" for="operational-log-retention">
            <div class="field-title-row">
              <span>{tr('Request log retention')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.request_log_retention_days);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-log-retention" type="number" min="1" max="365" step="1" bind:value={draft.request_log_retention_days} disabled={busy !== ''} /><small>{tr('days')}</small></span>
          </label>
          <label class="resource-limit-field" for="operational-log-rows">
            <div class="field-title-row">
              <span>{tr('Maximum request log rows')}</span>
              <button
                type="button"
                class="config-help-trigger"
                title={tr('Config guidance and error codes')}
                aria-label={tr('Config guidance and error codes')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openHelp(coreHelpMap.request_log_max_rows);
                }}
              >
                <CircleHelp size={13.5} />
              </button>
            </div>
            <span class="resource-limit-input"><input id="operational-log-rows" type="number" min="1" max="100000" step="1" bind:value={draft.request_log_max_rows} disabled={busy !== ''} /><small>{tr('rows')}</small></span>
            <small>{tr('The maximum remains 100000 rows.')}</small>
          </label>
          <div class="settings-subsection-heading">
            <h3>{tr('Upstream limits')}</h3>
            <p>{tr('Configure retry, stream continuity, provider usage, media, and client-cache limits.')}</p>
          </div>
          {#each upstreamFields as field}
            <label class="resource-limit-field" for={`operational-upstream-${field.key}`}>
              <div class="field-title-row">
                <span>{tr(field.label)}</span>
                <button
                  type="button"
                  class="config-help-trigger"
                  title={tr('Config guidance and error codes')}
                  aria-label={tr('Config guidance and error codes')}
                  onclick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    openHelp({
                      titleKey: field.label,
                      descKey: field.descKey,
                      violationKey: field.violationKey,
                      httpStatus: field.httpStatus,
                      errorCode: field.errorCode,
                      defaultVal: field.defaultVal,
                      safeRange: field.safeRange,
                    });
                  }}
                >
                  <CircleHelp size={13.5} />
                </button>
              </div>
              <span class="resource-limit-input">
                <input
                  id={`operational-upstream-${field.key}`}
                  type="number"
                  min={field.min}
                  max={field.max}
                  step="1"
                  value={draft.upstream[field.key]}
                  oninput={(event) => updateUpstream(field.key, event)}
                  disabled={busy !== ''}
                />
                <small>{tr(field.unit)}</small>
              </span>
            </label>
          {/each}
          <small class="settings-subsection-note">{tr('Backend validation keeps every upstream value within a bounded safe range.')}</small>
        </div>

        <div class="resource-limits-actions">
          <button class="primary-button" type="submit" disabled={busy !== '' || !draftIsValid || !hasChanges}>
            {#if busy === 'saving'}<LoaderCircle size={14} class="spin" />{tr('Saving…')}{:else}<Save size={14} />{tr('Save settings')}{/if}
          </button>
          <button class="secondary-button" type="button" onclick={resetToEnvironment} disabled={busy !== '' || !snapshot.overridden}>
            {#if busy === 'resetting'}<LoaderCircle size={14} class="spin" />{tr('Resetting…')}{:else}<RotateCcw size={14} />{tr('Reset to environment/default')}{/if}
          </button>
          {#if !draftIsValid}<span class="resource-limits-validation">{tr('One or more values are outside their allowed range.')}</span>{/if}
        </div>
      </form>
    </div>
  {/if}
</section>

<ArkDialog
  open={unlimitedWarningOpen}
  closeLabel={tr('Close dialog')}
  title={tr('Confirm unlimited gateway concurrency')}
  kicker={tr('Resource limit warning')}
  onClose={() => unlimitedWarningOpen = false}
>
  <div class="modal-form">
    <div class="resource-limits-notice" role="alert">{tr('Unlimited total gateway concurrency removes the process-wide admission cap. It can exhaust RAM, CPU, or file descriptors. Per-provider limits, API key rate limits, body limits, and SSE limits still apply.')}</div>
    <p class="modal-description">{tr('Apply unlimited gateway concurrency now?')}</p>
    <div class="modal-actions">
      <button type="button" class="secondary-button" onclick={() => unlimitedWarningOpen = false}>{tr('Cancel')}</button>
      <button type="button" class="primary-button" onclick={confirmUnlimited}>{tr('Apply unlimited gateway concurrency')}</button>
    </div>
  </div>
</ArkDialog>

  <ConfigHelpDialog {tr} help={activeHelp} onClose={() => { activeHelp = null; }} />
