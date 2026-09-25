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
  import SettingsField from './SettingsField.svelte';
  import { coreHelpMap, coreLimitFields, coreTimeoutFields, defaults, isValidOperationalSettings, sameOperationalSettings, upstreamFields, type ConfigHelp, type UpstreamKey } from './operational-settings.config';

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
  let active = true;
  let requestGeneration = 0;
  let observedRefreshToken = 0;

  function openHelp(next: ConfigHelp): void {
    activeHelp = next;
  }

  $: draftIsValid = isValidOperationalSettings(draft);
  $: hasChanges = snapshot !== null
    && (!snapshot.overridden || !sameOperationalSettings(draft, snapshot.settings));
  $: if (refreshToken > observedRefreshToken) {
    observedRefreshToken = refreshToken;
    void load();
  }

  function updateUpstream(key: UpstreamKey, event: Event): void {
    const value = Number((event.currentTarget as HTMLInputElement).value);
    draft = { ...draft, upstream: { ...draft.upstream, [key]: value } };
  }

  function fieldHelp(field: (typeof upstreamFields)[number]): ConfigHelp {
    return {
      titleKey: field.label,
      descKey: field.descKey,
      violationKey: field.violationKey,
      httpStatus: field.httpStatus,
      errorCode: field.errorCode,
      defaultVal: field.defaultVal,
      safeRange: field.safeRange,
    };
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
          if (latest.overridden && sameOperationalSettings(latest.settings, values)) {
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
          {#each coreTimeoutFields as field (field.key)}
            <SettingsField
              tr={tr}
              id={field.id}
              title={tr(field.labelKey)}
              bind:value={draft[field.key]}
              min={field.min}
              max={field.max}
              unit={field.unit ?? (field.unitKey ? tr(field.unitKey) : '')}
              hint={field.hintKey ? tr(field.hintKey) : ''}
              help={coreHelpMap[field.key]}
              onHelp={openHelp}
              disabled={busy !== ''}
            />
          {/each}
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
          {#each coreLimitFields as field (field.key)}
            <SettingsField
              tr={tr}
              id={field.id}
              title={tr(field.labelKey)}
              bind:value={draft[field.key]}
              min={field.min}
              max={field.max}
              unit={field.unit ?? (field.unitKey ? tr(field.unitKey) : '')}
              hint={field.hintKey ? tr(field.hintKey) : ''}
              help={coreHelpMap[field.key]}
              onHelp={openHelp}
              disabled={busy !== ''}
            />
          {/each}
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
                    openHelp(fieldHelp(field));
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
