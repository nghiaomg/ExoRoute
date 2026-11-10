<script lang="ts">
  import { ShieldAlert, SlidersHorizontal } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { GatewayApiKey } from '../../lib/types';
  import ApiKeyScopeEditor from './ApiKeyScopeEditor.svelte';
  import ApiKeyUsagePanel from './ApiKeyUsagePanel.svelte';
  import { formatUtcTimestamp } from './format-utc';

  interface Props {
    /** The key being inspected, or null when the sheet is closed. */
    apiKey: GatewayApiKey | null;
    tr: Translate;
    locale: Locale;
    /** Called after a scope save so the page can refresh the key list. */
    onUpdated: () => void;
    onClose: () => void;
  }
  let { apiKey, tr, locale, onUpdated, onClose }: Props = $props();

  let dialog = $state<HTMLDialogElement | null>(null);
  let scopeOpen = $state(false);

  // Opening is driven by the prop so the page owns which key is selected; a
  // closed sheet reports back through onClose instead of clearing the prop.
  $effect(() => {
    const element = dialog;
    if (!element) return;
    if (apiKey && !element.open) {
      element.showModal();
      scopeOpen = false;
    } else if (!apiKey && element.open) {
      element.close();
    }
  });

  const createdLabel = $derived(formatUtcTimestamp(apiKey?.created_at, locale));
  const lastUsedLabel = $derived(
    apiKey?.last_used_at ? formatUtcTimestamp(apiKey.last_used_at, locale) : null,
  );
</script>

<dialog class="modal key-detail-modal" bind:this={dialog} aria-labelledby="api-key-detail-title" onclose={onClose}>
  <div class="modal-card key-detail">
    {#if apiKey}
      <div class="modal-heading">
        <div>
          <h2 id="api-key-detail-title">{tr('Key details')}</h2>
          <p>{apiKey.name}</p>
        </div>
        <button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</button>
      </div>

      {#if apiKey.scope_valid === false}
        <p class="key-alert" role="alert">
          <ShieldAlert size={14} />
          {tr('The stored access scope is invalid, so every request with this key is rejected. Save a scope to repair it.')}
        </p>
      {/if}

      <dl class="key-facts">
        <div><dt>{tr('Status')}</dt><dd>{apiKey.enabled ? tr('Enabled') : tr('Disabled')}</dd></div>
        <div><dt>{tr('API requests')}</dt><dd>{apiKey.request_count ?? 0}</dd></div>
        <div><dt>{tr('Created')}</dt><dd>{createdLabel ?? apiKey.created_at}</dd></div>
        <div><dt>{tr('Last used')}</dt><dd>{lastUsedLabel ?? tr('Never used')}</dd></div>
        <div class="key-facts-id"><dt>{tr('API key ID')}</dt><dd class="mono">{apiKey.id}</dd></div>
      </dl>

      <!-- The scope summary lives on the toggle itself rather than in the fact
           grid, so the state and the control that changes it stay together. -->
      <button
        class="scope-toggle"
        type="button"
        aria-expanded={scopeOpen}
        aria-controls="api-key-scope-section"
        onclick={() => (scopeOpen = !scopeOpen)}
      >
        <SlidersHorizontal size={14} />
        {tr('Access scope')}
        <span class="scope-toggle-hint">{apiKey.scope_restricted ? tr('restricted') : tr('all models')}</span>
      </button>

      {#if scopeOpen}
        <section id="api-key-scope-section" class="scope-section">
          <ApiKeyScopeEditor {apiKey} {tr} {onUpdated} embedded />
        </section>
      {/if}

      <ApiKeyUsagePanel apiKeyId={apiKey.id} {tr} {locale} />
    {/if}

    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>{tr('Close')}</button>
    </div>
  </div>
</dialog>

<style>
  .key-detail {
    gap: 13px;
  }
  .key-facts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 8px;
    margin: 0;
  }
  .key-facts > div {
    display: grid;
    gap: 2px;
    min-width: 0;
    padding: 9px 11px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface-sunken);
  }
  /* The ID is long and opaque, so it spans the row and truncates instead of
     stretching the fact grid. */
  .key-facts-id {
    grid-column: 1 / -1;
  }
  .key-facts dt {
    color: var(--muted);
    font-size: 11px;
  }
  .key-facts dd {
    margin: 0;
    overflow-wrap: anywhere;
    color: var(--ink);
    font-size: 12px;
    font-weight: 600;
  }
  .key-facts dd.mono {
    overflow: hidden;
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope-toggle {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    padding: 9px 11px;
    color: var(--ink);
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface);
    font: 600 12px var(--font-sans);
    text-align: left;
    cursor: pointer;
  }
  .scope-toggle:hover {
    border-color: var(--accent);
  }
  .scope-toggle-hint {
    margin-left: auto;
    color: var(--muted);
    font-weight: 500;
  }
  .scope-section {
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface-sunken);
  }
  .key-alert {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    margin: 0;
    padding: 9px 11px;
    color: #b45309;
    border: 1px solid rgba(180, 83, 9, 0.28);
    border-radius: 9px;
    background: rgba(249, 115, 22, 0.08);
    font-size: 12px;
    line-height: 1.45;
  }

  /* The sheet holds facts, scope and a usage panel, so it takes the wide
     dialog width rather than the design system's 440px default. */
  :global(.dashboard-app .modal.key-detail-modal) {
    width: min(94vw, 660px);
  }
</style>
