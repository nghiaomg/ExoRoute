<script lang="ts">
  import { onDestroy } from 'svelte';
  import { Check, Copy } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import type { Locale } from '../../lib/i18n';
  import { formatDate, type Translate } from '../../lib/format';
  import type { RequestLog } from '../../lib/types';

  export let tr: Translate;
  export let locale: Locale;
  export let request: RequestLog | null = null;
  export let onClose: () => void;

  let errorCopied = false;
  let copyTimer: number | null = null;

  function close(): void {
    errorCopied = false;
    if (copyTimer !== null) {
      window.clearTimeout(copyTimer);
      copyTimer = null;
    }
    onClose();
  }

  async function copyErrorText(): Promise<void> {
    if (!request?.error) return;
    try {
      await navigator.clipboard.writeText(request.error);
      errorCopied = true;
      if (copyTimer !== null) window.clearTimeout(copyTimer);
      copyTimer = window.setTimeout(() => {
        errorCopied = false;
        copyTimer = null;
      }, 2000);
    } catch {}
  }

  onDestroy(() => {
    if (copyTimer !== null) window.clearTimeout(copyTimer);
  });
</script>

<ArkDialog
  open={request !== null}
  role="dialog"
  title={tr('Request error details')}
  kicker={tr('REQUEST DIAGNOSTICS')}
  closeLabel={tr('Close dialog')}
  wide
  onClose={close}
>
  {#if request}
    <div class="modal-form request-error-modal">
      <div class="request-error-meta-grid">
        <div class="error-meta-card">
          <span class="error-meta-label">{tr('TIME')}</span>
          <span class="error-meta-val">{formatDate(request.created_at, locale)}</span>
        </div>
        <div class="error-meta-card">
          <span class="error-meta-label">{tr('MODEL')}</span>
          <span class="error-meta-val font-mono" title={request.model}>{request.model}</span>
        </div>
        <div class="error-meta-card">
          <span class="error-meta-label">{tr('STATUS')}</span>
          <span class="status-badge failure">{request.status ?? '—'}</span>
        </div>
        <div class="error-meta-card">
          <span class="error-meta-label">{tr('PROVIDER')}</span>
          <span class="error-meta-val">{request.provider_id ?? '—'}</span>
        </div>
        {#if request.route_alias}
          <div class="error-meta-card">
            <span class="error-meta-label">{tr('ALIAS / COMBO')}</span>
            <span class="error-meta-val">{request.route_alias}</span>
          </div>
        {/if}
        {#if request.provider_credential_id}
          <div class="error-meta-card error-meta-full">
            <span class="error-meta-label">{tr('Provider credential')}</span>
            <code class="error-meta-code">{request.provider_credential_id}</code>
          </div>
        {/if}
      </div>

      <div class="request-error-box">
        <div class="request-error-box-header">
          <span class="request-error-box-title">{tr('Error message')}</span>
          <button type="button" class="secondary-button compact copy-error-button" onclick={copyErrorText} title={tr('Copy error')}>
            {#if errorCopied}
              <Check size={13} color="#16a34a" />
              <span>{tr('Copied!')}</span>
            {:else}
              <Copy size={13} />
              <span>{tr('Copy error')}</span>
            {/if}
          </button>
        </div>
        <pre class="request-error-pre">{request.error}</pre>
      </div>

      <div class="modal-actions" style="margin-top: 18px;">
        <button type="button" class="secondary-button" onclick={close}>{tr('Close')}</button>
      </div>
    </div>
  {/if}
</ArkDialog>

<style>
  .request-error-modal {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .request-error-meta-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
    gap: 10px;
    padding: 12px;
    background: #f8f9fc;
    border: 1px solid #ebedf5;
    border-radius: 10px;
  }
  :global(:root[data-theme='dark']) .request-error-meta-grid {
    background: #1c1d2c;
    border-color: #292b3f;
  }
  .error-meta-card {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .error-meta-full { grid-column: 1 / -1; }
  .error-meta-label {
    font-size: 10.5px;
    font-weight: 700;
    color: #8b90a4;
    letter-spacing: 0.5px;
  }
  .error-meta-val {
    font-size: 12.5px;
    color: #2d3142;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  :global(:root[data-theme='dark']) .error-meta-val { color: #e2e4ef; }
  .error-meta-code {
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: #c2410c;
    word-break: break-all;
  }
  .request-error-box {
    display: flex;
    flex-direction: column;
    border: 1px solid #fed7aa;
    border-radius: 10px;
    overflow: hidden;
    background: #fffaf5;
  }
  :global(:root[data-theme='dark']) .request-error-box {
    border-color: #6c2e17;
    background: #1f1816;
  }
  .request-error-box-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    background: #ffedd5;
    border-bottom: 1px solid #fed7aa;
  }
  :global(:root[data-theme='dark']) .request-error-box-header {
    background: #2e1d18;
    border-bottom-color: #552414;
  }
  .request-error-box-title {
    font-size: 12px;
    font-weight: 700;
    color: #9a3412;
  }
  :global(:root[data-theme='dark']) .request-error-box-title { color: #fdba74; }
  .copy-error-button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 10px;
    font-size: 11.5px;
    border-radius: 6px;
  }
  .request-error-pre {
    margin: 0;
    padding: 12px 14px;
    max-height: 280px;
    overflow-y: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.5;
    color: #7f1d1d;
    user-select: text;
  }
  :global(:root[data-theme='dark']) .request-error-pre { color: #fecaca; }
</style>
