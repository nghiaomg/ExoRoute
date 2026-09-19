<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    CircleHelp,
    LoaderCircle,
    RefreshCw,
    Save,
    ShieldCheck,
    SlidersHorizontal,
    Sparkles,
    Zap,
  } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import { ApiError, api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { GatewayResourceLimitValues, GatewayResourceLimits } from '../../lib/types';

  export let tr: Translate;
  export let refreshToken = 0;

  let limits: GatewayResourceLimits | null = null;
  let draft: GatewayResourceLimitValues = {
    gateway_body_limit_mib: 16,
    gateway_body_processing_concurrency: 4,
    sse_frame_limit_kib: 1024,
    sse_buffer_limit_kib: 2048,
    provider_max_concurrency: 32,
    stream_continuity_enabled: false,
    stream_continuity_max_concurrency: 16,
  };
  let busy: '' | 'loading' | 'saving' = 'loading';
  let errorMessage = '';
  let successMessage = '';
  let reloadConflict = false;
  let unlimitedWarningOpen = false;
  let unlimitedProviderWarningOpen = false;
  let continuityHelpOpen = false;
  let pendingUnlimitedBodyWarning = false;
  let pendingUnlimitedProviderWarning = false;
  let confirmedUnlimitedProviderConcurrency = false;
  let active = true;
  let requestGeneration = 0;
  let observedRefreshToken = 0;
  const bodyProcessingChoices = [1, 2, 3, 4, 5, 6, 7, 8];

  $: draftIsValid = Number.isSafeInteger(draft.gateway_body_limit_mib)
    && draft.gateway_body_limit_mib >= 1
    && draft.gateway_body_limit_mib <= 16
    && Number.isSafeInteger(draft.gateway_body_processing_concurrency)
    && (draft.gateway_body_processing_concurrency === 0
      || (draft.gateway_body_processing_concurrency >= 1
        && draft.gateway_body_processing_concurrency <= 8))
    && Number.isSafeInteger(draft.sse_frame_limit_kib)
    && (draft.sse_frame_limit_kib === 0 || draft.sse_frame_limit_kib >= 64)
    && Number.isSafeInteger(draft.sse_buffer_limit_kib)
    && (draft.sse_buffer_limit_kib === 0
      || (draft.sse_frame_limit_kib !== 0
        && draft.sse_buffer_limit_kib >= draft.sse_frame_limit_kib))
    && Number.isSafeInteger(draft.provider_max_concurrency)
    && draft.provider_max_concurrency >= 0
    && draft.provider_max_concurrency <= 32
    && Number.isSafeInteger(draft.stream_continuity_max_concurrency)
    && draft.stream_continuity_max_concurrency >= 1
    && draft.stream_continuity_max_concurrency <= 64;
  $: hasChanges = limits !== null && (
    draft.gateway_body_limit_mib !== limits.saved.gateway_body_limit_mib
    || draft.gateway_body_processing_concurrency !== limits.saved.gateway_body_processing_concurrency
    || draft.sse_frame_limit_kib !== limits.saved.sse_frame_limit_kib
    || draft.sse_buffer_limit_kib !== limits.saved.sse_buffer_limit_kib
    || draft.provider_max_concurrency !== limits.saved.provider_max_concurrency
    || draft.stream_continuity_enabled !== limits.saved.stream_continuity_enabled
    || draft.stream_continuity_max_concurrency !== limits.saved.stream_continuity_max_concurrency
  );
  $: if (refreshToken > observedRefreshToken) {
    observedRefreshToken = refreshToken;
    void load();
  }

  function sameLimitValues(left: GatewayResourceLimitValues, right: GatewayResourceLimitValues): boolean {
    return left.gateway_body_limit_mib === right.gateway_body_limit_mib
      && left.gateway_body_processing_concurrency === right.gateway_body_processing_concurrency
      && left.sse_frame_limit_kib === right.sse_frame_limit_kib
      && left.sse_buffer_limit_kib === right.sse_buffer_limit_kib
      && left.provider_max_concurrency === right.provider_max_concurrency
      && left.stream_continuity_enabled === right.stream_continuity_enabled
      && left.stream_continuity_max_concurrency === right.stream_continuity_max_concurrency;
  }

  async function load(): Promise<void> {
    const currentRequest = ++requestGeneration;
    busy = 'loading';
    errorMessage = '';
    successMessage = '';
    reloadConflict = false;
    unlimitedWarningOpen = false;
    unlimitedProviderWarningOpen = false;
    pendingUnlimitedBodyWarning = false;
    pendingUnlimitedProviderWarning = false;
    confirmedUnlimitedProviderConcurrency = false;
    try {
      const result = await api.gatewayResourceLimits();
      if (!active || currentRequest !== requestGeneration) return;
      limits = result;
      draft = { ...result.saved };
    } catch (error) {
      if (!active || currentRequest !== requestGeneration) return;
      errorMessage = localizedError(error, 'Could not load gateway resource limits.', tr);
    } finally {
      if (active && currentRequest === requestGeneration) busy = '';
    }
  }

  async function applyDraft(confirmUnlimitedProviderConcurrency: boolean): Promise<void> {
    if (busy || !draftIsValid || !hasChanges) return;
    const requestedValues = { ...draft };
    const expectedSaved = { ...limits!.saved };
    busy = 'saving';
    errorMessage = '';
    successMessage = '';
    reloadConflict = false;
    try {
      const result = await api.updateGatewayResourceLimits(
        requestedValues,
        expectedSaved,
        confirmUnlimitedProviderConcurrency
          || (requestedValues.provider_max_concurrency === 0
            && expectedSaved.provider_max_concurrency === 0),
      );
      if (!active) return;
      limits = result;
      draft = { ...result.saved };
      successMessage = tr('Gateway settings saved and applied immediately.');
    } catch (error) {
      if (!active) return;
      if (error instanceof ApiError && error.status === 0) {
        try {
          const latest = await api.gatewayResourceLimits();
          if (!active) return;
          limits = latest;
          if (sameLimitValues(latest.saved, requestedValues)
            && sameLimitValues(latest.active, requestedValues)) {
            draft = { ...latest.saved };
            successMessage = tr('Gateway settings were saved; the response was interrupted.');
          } else {
            reloadConflict = !sameLimitValues(latest.saved, expectedSaved);
            errorMessage = localizedError(error, 'Could not save gateway resource limits.', tr);
          }
        } catch {
          errorMessage = localizedError(error, 'Could not save gateway resource limits.', tr);
        }
      } else {
        errorMessage = localizedError(error, 'Could not save gateway resource limits.', tr);
        reloadConflict = error instanceof ApiError && error.status === 409;
      }
    } finally {
      if (active) busy = '';
    }
  }

  function save(event: SubmitEvent): void {
    event.preventDefault();
    if (busy || !draftIsValid || !hasChanges) return;
    pendingUnlimitedBodyWarning = draft.gateway_body_processing_concurrency === 0
      && limits?.saved.gateway_body_processing_concurrency !== 0;
    pendingUnlimitedProviderWarning = draft.provider_max_concurrency === 0
      && limits?.saved.provider_max_concurrency !== 0;
    confirmedUnlimitedProviderConcurrency = false;
    openNextUnlimitedWarning();
  }

  async function confirmUnlimitedProcessing(): Promise<void> {
    unlimitedWarningOpen = false;
    pendingUnlimitedBodyWarning = false;
    await continueAfterUnlimitedWarning();
  }

  async function confirmUnlimitedProviderConcurrency(): Promise<void> {
    unlimitedProviderWarningOpen = false;
    pendingUnlimitedProviderWarning = false;
    confirmedUnlimitedProviderConcurrency = true;
    await continueAfterUnlimitedWarning();
  }

  function openNextUnlimitedWarning(): void {
    if (pendingUnlimitedProviderWarning) {
      unlimitedProviderWarningOpen = true;
      return;
    }
    if (pendingUnlimitedBodyWarning) {
      unlimitedWarningOpen = true;
      return;
    }
    const confirmedProviderLimit = confirmedUnlimitedProviderConcurrency;
    confirmedUnlimitedProviderConcurrency = false;
    void applyDraft(confirmedProviderLimit);
  }

  async function continueAfterUnlimitedWarning(): Promise<void> {
    if (pendingUnlimitedProviderWarning || pendingUnlimitedBodyWarning) {
      openNextUnlimitedWarning();
      return;
    }
    const confirmedProviderLimit = confirmedUnlimitedProviderConcurrency;
    confirmedUnlimitedProviderConcurrency = false;
    await applyDraft(confirmedProviderLimit);
  }

  function cancelUnlimitedWarnings(): void {
    unlimitedWarningOpen = false;
    unlimitedProviderWarningOpen = false;
    pendingUnlimitedBodyWarning = false;
    pendingUnlimitedProviderWarning = false;
    confirmedUnlimitedProviderConcurrency = false;
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
      <h2>{tr('Gateway resource limits')}</h2>
      <p>{tr('Tune the maximum memory and request concurrency used by gateway traffic.')}</p>
    </div>
  </div>

  {#if busy === 'loading' && limits === null}
    <div class="resource-limits-state" aria-live="polite"><LoaderCircle size={15} class="spin" />{tr('Loading resource limits…')}</div>
  {:else if errorMessage && limits === null}
    <div class="resource-limits-state resource-limits-error" role="alert">
      <span>{errorMessage}</span>
      <button class="secondary-button compact" type="button" onclick={load}>{tr('Retry')}</button>
    </div>
  {:else if limits}
    <div class="resource-limits-content">
      <p class="resource-limits-description">{tr('New requests use updated limits immediately. Existing requests keep their settings; active continuity tasks keep running.')}</p>

      {#if errorMessage}
        <div class="resource-limits-error" role="alert">
          <span>{errorMessage}</span>
          {#if reloadConflict}<button class="secondary-button compact" type="button" onclick={load}>{tr('Reload latest settings')}</button>{/if}
        </div>
      {/if}
      {#if successMessage}<div class="resource-limits-success" role="status">{successMessage}</div>{/if}

      <div class="resource-limits-active">
        <strong>{tr('Currently active')}</strong>
        <span>{tr('Body {body} MiB · body-processing slots {bodySlots} · SSE frame {frame} · SSE buffer {buffer} · Provider concurrency {concurrency} · Continuity {continuity}', {
          body: limits.active.gateway_body_limit_mib,
          bodySlots: limits.active.gateway_body_processing_concurrency === 0
            ? tr('Unlimited')
            : limits.active.gateway_body_processing_concurrency,
           frame: limits.active.sse_frame_limit_kib === 0
             ? tr('Unlimited')
             : `${limits.active.sse_frame_limit_kib} KiB`,
           buffer: limits.active.sse_buffer_limit_kib === 0
             ? tr('Unlimited')
             : `${limits.active.sse_buffer_limit_kib} KiB`,
          concurrency: limits.active.provider_max_concurrency === 0
            ? tr('Unlimited')
            : limits.active.provider_max_concurrency,
          continuity: tr(limits.active.stream_continuity_enabled ? 'Enabled' : 'Disabled'),
        })}</span>
        <small>{tr('Continuity capacity: {capacity} tasks', { capacity: limits.active.stream_continuity_max_concurrency })}</small>
      </div>

      <form class="resource-limits-form" onsubmit={save}>
        <div class="resource-limits-grid">
          <label class="resource-limit-field" for="gateway-body-limit">
            <span>{tr('Gateway request body')}</span>
            <span class="resource-limit-input"><input id="gateway-body-limit" type="number" min="1" max="16" step="1" bind:value={draft.gateway_body_limit_mib} disabled={busy !== ''} /><small>MiB</small></span>
            <small>{tr('Maximum accepted request body size (1–16 MiB).')}</small>
          </label>
          <label class="resource-limit-field" for="gateway-body-processing-concurrency">
            <span>{tr('Concurrent bodies being processed')}</span>
            <span class="resource-limit-input">
              <select id="gateway-body-processing-concurrency" bind:value={draft.gateway_body_processing_concurrency} disabled={busy !== ''}>
                <option value={0}>{tr('Unlimited')}</option>
                {#each bodyProcessingChoices as concurrency}
                  <option value={concurrency}>{concurrency} {tr('requests')}</option>
                {/each}
              </select>
            </span>
            <small>{tr('Choose 1–8 concurrent requests or unlimited processing.')}</small>
            <small>{tr('Unlimited processing can use all available CPU and memory under heavy traffic.')}</small>
            <small>{tr('SSE frame parsing uses the same concurrency limit.')}</small>
          </label>
          <label class="resource-limit-field" for="sse-frame-limit">
            <span>{tr('SSE frame size')}</span>
            <span class="resource-limit-input"><input id="sse-frame-limit" type="number" min="0" step="1" bind:value={draft.sse_frame_limit_kib} disabled={busy !== ''} /><small>KiB</small></span>
            <small>{tr('Maximum size of one streamed event (64 KiB–1 MiB).')}</small>
          </label>
          <label class="resource-limit-field" for="sse-buffer-limit">
            <span>{tr('SSE buffer size')}</span>
            <span class="resource-limit-input"><input id="sse-buffer-limit" type="number" min="0" step="1" bind:value={draft.sse_buffer_limit_kib} disabled={busy !== ''} /><small>KiB</small></span>
            <small>{tr('Maximum buffered stream data (at least the frame size, up to 2 MiB).')}</small>
          </label>
          <label class="resource-limit-field" for="provider-concurrency-limit">
            <span>{tr('Concurrent requests per provider')}</span>
            <span class="resource-limit-input"><input id="provider-concurrency-limit" type="number" min="0" max="32" step="1" bind:value={draft.provider_max_concurrency} disabled={busy !== ''} /><small>{tr('requests')}</small></span>
            <small>{tr('Set to 0 for unlimited; finite values allow 1–32 concurrent requests per provider.')}</small>
          </label>
          <label class="resource-limit-field" for="stream-continuity-concurrency-limit">
            <span>{tr('Concurrent continuity tasks')}</span>
            <span class="resource-limit-input"><input id="stream-continuity-concurrency-limit" type="number" min="1" max="64" step="1" bind:value={draft.stream_continuity_max_concurrency} disabled={busy !== ''} /><small>{tr('tasks')}</small></span>
            <small>{tr('Choose 1–64 concurrent continuity tasks. This limit is always finite; 0 is not unlimited.')}</small>
          </label>
          <div class="resource-limit-field resource-continuity-field">
            <div class="resource-continuity-title-row">
              <label for="stream-continuity-enabled" class="resource-continuity-label-text">
                {tr('Stream continuity')}
              </label>
              <button
                type="button"
                class="continuity-help-trigger"
                title={tr('Learn more about stream continuity')}
                aria-label={tr('Learn more about stream continuity')}
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  continuityHelpOpen = true;
                }}
              >
                <CircleHelp size={14} />
              </button>
            </div>
            <label class="resource-continuity-control" for="stream-continuity-enabled">
              <input id="stream-continuity-enabled" type="checkbox" bind:checked={draft.stream_continuity_enabled} disabled={busy !== ''} />
              <strong>{tr(draft.stream_continuity_enabled ? 'Enabled' : 'Disabled')}</strong>
            </label>
            <small>{tr('Keep streamed work running after a client disconnects, for up to 24 hours. Reconnect by stream ID or send DELETE to cancel it.')}</small>
            <small>{tr('The configured continuity capacity is always finite. Replay is bounded; reaching its storage limit does not stop the task, but missed events may not be resumable.')}</small>
          </div>
        </div>

        <div class="resource-limits-actions">
          <button class="primary-button" type="submit" disabled={busy !== '' || !draftIsValid || !hasChanges}>
            {#if busy === 'saving'}<LoaderCircle size={14} class="spin" />{tr('Saving…')}{:else}<Save size={14} />{tr('Save settings')}{/if}
          </button>
           {#if !draftIsValid}<span class="resource-limits-validation">{tr('Set 0 for unlimited, or ensure the finite buffer is at least the finite frame size.')}</span>{/if}
        </div>
      </form>
    </div>
  {/if}
</section>

<ArkDialog
  open={unlimitedWarningOpen}
  closeLabel={tr('Close dialog')}
  title={tr('Confirm unlimited processing')}
  kicker={tr('Resource limit warning')}
  onClose={cancelUnlimitedWarnings}
>
  <div class="modal-form">
    <div class="resource-limits-notice" role="alert">{tr('Unlimited processing removes the separate body and SSE frame concurrency cap. Body and stream-size limits and the overall gateway/provider limits still apply, but high traffic can increase CPU and memory use enough to exhaust this device.')}</div>
    <p class="modal-description">{tr('Apply unlimited body and SSE processing now?')}</p>
    <div class="modal-actions">
      <button type="button" class="secondary-button" onclick={cancelUnlimitedWarnings}>{tr('Cancel')}</button>
      <button type="button" class="primary-button" onclick={confirmUnlimitedProcessing}>{tr('Apply unlimited processing')}</button>
    </div>
  </div>
</ArkDialog>

<ArkDialog
  open={unlimitedProviderWarningOpen}
  closeLabel={tr('Close dialog')}
  title={tr('Confirm unlimited provider concurrency')}
  kicker={tr('Resource limit warning')}
  onClose={cancelUnlimitedWarnings}
>
  <div class="modal-form">
    <div class="resource-limits-notice" role="alert">{tr('Unlimited per-provider concurrency removes the per-provider request cap. A single provider can consume all available gateway capacity and exhaust RAM, CPU, or file descriptors. Overall gateway concurrency, API-key rate limits, body and SSE limits, timeouts, and circuit breakers still apply.')}</div>
    <p class="modal-description">{tr('Apply unlimited concurrency for each provider now?')}</p>
    <div class="modal-actions">
      <button type="button" class="secondary-button" onclick={cancelUnlimitedWarnings}>{tr('Cancel')}</button>
      <button type="button" class="primary-button" onclick={confirmUnlimitedProviderConcurrency}>{tr('Apply unlimited provider concurrency')}</button>
    </div>
  </div>
</ArkDialog>

<ArkDialog
  open={continuityHelpOpen}
  wide
  closeLabel={tr('Close dialog')}
  title={tr('Stream Continuity Guide')}
  kicker={tr('Gateway Architecture')}
  class="continuity-guide-dialog"
  onClose={() => { continuityHelpOpen = false; }}
>
  <div class="continuity-guide-container">
    <div class="continuity-hero">
      <div class="continuity-hero-badge">
        <Sparkles size={14} />
        <span>{tr('Resilient Streaming & Zero-Loss Recovery')}</span>
      </div>
      <p class="continuity-hero-desc">
        {tr('Stream Continuity keeps AI inference streams running in the background when client connections drop, buffering tokens into local storage so they can be resumed seamlessly without loss.')}
      </p>
    </div>

    <div class="continuity-section">
      <h3 class="continuity-section-title">{tr('Key Benefits')}</h3>
      <div class="continuity-grid">
        <div class="continuity-card">
          <div class="continuity-card-icon is-emerald">
            <ShieldCheck size={18} />
          </div>
          <div class="continuity-card-body">
            <h4>{tr('Zero Data Loss on Disconnect')}</h4>
            <p>{tr('Long generations, complex code generation, or multi-step reasoning often take minutes. If your Wi-Fi flickers, IDE reloads, or proxy times out, the stream continues running and 100% of generated tokens are preserved.')}</p>
          </div>
        </div>

        <div class="continuity-card">
          <div class="continuity-card-icon is-amber">
            <Zap size={18} />
          </div>
          <div class="continuity-card-body">
            <h4>{tr('Cost & Time Efficiency')}</h4>
            <p>{tr('Avoid double-billing and wasted tokens. You never need to re-run expensive prompts from scratch because a transient network blip interrupted the response.')}</p>
          </div>
        </div>

        <div class="continuity-card">
          <div class="continuity-card-icon is-blue">
            <RefreshCw size={18} />
          </div>
          <div class="continuity-card-body">
            <h4>{tr('Automatic Upstream Retry')}</h4>
            <p>{tr('If an upstream provider encounters transient errors (HTTP 429 rate limits, 5xx server errors, or timeouts), ExoRoute automatically retries the background task before failing.')}</p>
          </div>
        </div>
      </div>
    </div>

    <div class="continuity-section">
      <h3 class="continuity-section-title">{tr('How it Works')}</h3>
      <div class="continuity-steps">
        <div class="continuity-step">
          <div class="step-num">1</div>
          <div class="step-content">
            <h5>{tr('Stream Identification')}</h5>
            <p>{tr('Every streaming response includes an “x-exoroute-stream-id: <stream_id>” header and sequential SSE event IDs.')}</p>
            <pre class="continuity-code"><code>HTTP/1.1 200 OK
content-type: text/event-stream
x-exoroute-stream-id: run_01jb9w4...
id: 124</code></pre>
          </div>
        </div>

        <div class="continuity-step">
          <div class="step-num">2</div>
          <div class="step-content">
            <h5>{tr('Resume Stream')}</h5>
            <p>{tr('Send “GET /v1/streams/<stream_id>” with the “Last-Event-ID” header to replay missed chunks and continue receiving live tokens.')}</p>
            <pre class="continuity-code"><code>GET /v1/streams/run_01jb9w4...
Last-Event-ID: 124
Authorization: Bearer exo_live_...</code></pre>
          </div>
        </div>

        <div class="continuity-step">
          <div class="step-num">3</div>
          <div class="step-content">
            <h5>{tr('Explicit Cancellation')}</h5>
            <p>{tr('Send “DELETE /v1/streams/<stream_id>” when a user cancels the generation to immediately release upstream and gateway resources.')}</p>
            <pre class="continuity-code"><code>DELETE /v1/streams/run_01jb9w4...
Authorization: Bearer exo_live_...</code></pre>
          </div>
        </div>
      </div>
    </div>

    <div class="continuity-section">
      <h3 class="continuity-section-title">{tr('Safety & System Limits')}</h3>
      <ul class="continuity-limits">
        <li>
          <span class="continuity-limit-bullet"></span>
          <span>{tr('Up to {limit} concurrent background continuity tasks. This limit is always finite; 0 is unlimited only for body processing, provider concurrency, and SSE size settings.', { limit: limits?.active.stream_continuity_max_concurrency ?? 16 })}</span>
        </li>
        <li>
          <span class="continuity-limit-bullet"></span>
          <span>{tr('Bounded replay buffer: max 8 MiB or 50,000 events per stream (128 MiB gateway total)')}</span>
        </li>
        <li>
          <span class="continuity-limit-bullet"></span>
          <span>{tr('Completed streams are retained for up to 24 hours (max 128 finished runs) in embedded LMDB storage')}</span>
        </li>
        <li>
          <span class="continuity-limit-bullet"></span>
          <span>{tr('Per-request override available via “X-ExoRoute-Stream-Continuity: true”')}</span>
        </li>
      </ul>
    </div>

    <div class="modal-actions" style="margin-top: 24px;">
      <button type="button" class="primary-button" onclick={() => { continuityHelpOpen = false; }}>
        {tr('Understood')}
      </button>
    </div>
  </div>
</ArkDialog>
