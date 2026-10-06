<script lang="ts">
  import { RefreshCw, ShieldCheck, Zap } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import { type Translate } from '../../lib/format';

  export let tr: Translate;
  export let open = false;
  export let onClose: () => void = () => {};
  export let continuityCapacity = 16;
</script>

<ArkDialog
  {open}
  class="continuity-guide-dialog"
  closeLabel={tr('Close dialog')}
  title={tr('Stream Continuity Guide')}
  {onClose}
>
  <div class="continuity-guide-container">
    <div class="continuity-hero">
      <span class="continuity-hero-badge">{tr('Resilient Streaming & Zero-Loss Recovery')}</span>
      <p class="continuity-hero-desc">{tr('Stream Continuity keeps AI inference streams running in the background when client connections drop, buffering tokens into local storage so they can be resumed seamlessly without loss.')}</p>
    </div>

    <div class="continuity-section">
      <h4 class="continuity-section-title">{tr('Key Benefits')}</h4>
      <div class="continuity-grid">
        <div class="continuity-card">
          <div class="continuity-card-icon is-emerald"><ShieldCheck size={17} /></div>
          <div class="continuity-card-body">
            <h4>{tr('Zero Data Loss on Disconnect')}</h4>
            <p>{tr('Long generations, complex code generation, or multi-step reasoning often take minutes. If your Wi-Fi flickers, IDE reloads, or proxy times out, the stream continues running and 100% of generated tokens are preserved.')}</p>
          </div>
        </div>
        <div class="continuity-card">
          <div class="continuity-card-icon is-amber"><Zap size={17} /></div>
          <div class="continuity-card-body">
            <h4>{tr('Cost & Time Efficiency')}</h4>
            <p>{tr('Avoid double-billing and wasted tokens. You never need to re-run expensive prompts from scratch because a transient network blip interrupted the response.')}</p>
          </div>
        </div>
        <div class="continuity-card">
          <div class="continuity-card-icon is-blue"><RefreshCw size={17} /></div>
          <div class="continuity-card-body">
            <h4>{tr('Automatic Upstream Retry')}</h4>
            <p>{tr('If an upstream provider encounters transient errors (HTTP 429 rate limits, 5xx server errors, or timeouts), ExoRoute automatically retries the background task before failing.')}</p>
          </div>
        </div>
      </div>
    </div>

    <div class="continuity-section">
      <h4 class="continuity-section-title">{tr('How it Works')}</h4>
      <div class="continuity-steps">
        <div class="continuity-step">
          <span class="step-num">1</span>
          <div class="step-content">
            <h5>{tr('Stream Identification')}</h5>
            <p>{tr('Every streaming response includes an “x-exoroute-stream-id: <stream_id>” header and sequential SSE event IDs.')}</p>
            <pre class="continuity-code">x-exoroute-stream-id: &lt;stream_id&gt;</pre>
          </div>
        </div>
        <div class="continuity-step">
          <span class="step-num">2</span>
          <div class="step-content">
            <h5>{tr('Resume Stream')}</h5>
            <p>{tr('Send “GET /v1/streams/<stream_id>” with the “Last-Event-ID” header to replay missed chunks and continue receiving live tokens.')}</p>
            <pre class="continuity-code">GET /v1/streams/&lt;stream_id&gt;
Last-Event-ID: &lt;last_event_id&gt;</pre>
          </div>
        </div>
        <div class="continuity-step">
          <span class="step-num">3</span>
          <div class="step-content">
            <h5>{tr('Explicit Cancellation')}</h5>
            <p>{tr('Send “DELETE /v1/streams/<stream_id>” when a user cancels the generation to immediately release upstream and gateway resources.')}</p>
            <pre class="continuity-code">DELETE /v1/streams/&lt;stream_id&gt;</pre>
          </div>
        </div>
      </div>
    </div>

    <div class="continuity-section">
      <h4 class="continuity-section-title">{tr('Safety & System Limits')}</h4>
      <ul class="continuity-limits">
        <li>
          <span class="continuity-limit-bullet"></span>
          <span>{tr('Up to {limit} concurrent background continuity tasks. This limit is always finite; 0 is unlimited only for body processing, provider concurrency, and SSE size settings.', { limit: continuityCapacity })}</span>
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

    <div class="modal-actions">
      <button type="button" class="primary-button" onclick={onClose}>
        {tr('Understood')}
      </button>
    </div>
  </div>
</ArkDialog>
