<script lang="ts">
  import { KeyRound, Sliders, Trash2 } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';
  import type { Provider } from '../../lib/types';
  import { labelProtocol, protocolBadgeClass } from '../../lib/labels';
  import ProviderAvatar from './ProviderAvatar.svelte';

  export let provider: Provider;
  export let tr: Translate;
  export let deleting = false;
  export let onDetails: (provider: Provider) => void;
  export let onManageKeys: (provider: Provider) => void;
  export let onDelete: (provider: Provider) => void;

  $: managesOAuthAccounts = provider.capabilities?.oauth_accounts === true;
  $: isFreebuff = provider.id === 'freebuff' || provider.adapter_id === 'freebuff';
</script>

<div
  class="preset-card is-connected"
  role="button"
  tabindex="0"
  onkeydown={(event) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      onDetails(provider);
    }
  }}
  onclick={() => onDetails(provider)}
>
  <div class="preset-card-top">
    <div class="preset-card-icon">
      <ProviderAvatar
        name={provider.name}
        logoUrl={provider.logo_url}
        adapterId={provider.adapter_id}
        providerId={provider.id}
      />
    </div>
    <span class="preset-badge" class:connected-badge={provider.enabled}>
      {tr(provider.enabled ? 'Connected' : 'Disabled')}
    </span>
  </div>

  <div class="preset-card-body">
    <h4>{provider.name}</h4>
    <p title={provider.base_url}>{provider.base_url}</p>
    <div class="preset-labels" aria-label={tr('Provider labels')}>
      <span class="preset-label-badge protocol-badge {protocolBadgeClass(provider.preferred_protocol)}">
        {labelProtocol(provider.preferred_protocol, tr)}
      </span>
      <span class="preset-label-badge">
        {provider.api_key_count ?? (provider.api_key ? 1 : 0)} {tr(managesOAuthAccounts ? 'Connected accounts' : 'API keys')}
      </span>
      <span class="preset-label-badge">
        {provider.model_count ?? 0} {tr('saved models')}
      </span>
      {#if provider.invalid_api_key_count}
        <span class="preset-label-badge warning-label">
          {provider.invalid_api_key_count} {tr('invalid')}
        </span>
      {/if}
      {#if isFreebuff}
        <span class="preset-label-badge warning-label">
          {tr('High ban risk')}
        </span>
      {/if}
    </div>
  </div>

  <div class="preset-card-footer">
    <span class="preset-detail-btn">
      <Sliders size={13} />
      <span>{tr('Provider details')}</span>
    </span>

    <div class="preset-card-actions">
      {#if !managesOAuthAccounts}
        <button
          type="button"
          class="preset-action-icon"
          aria-label={tr('Manage keys for {name}', { name: provider.name })}
          title={tr('Manage keys')}
          onclick={(e) => { e.stopPropagation(); onManageKeys(provider); }}
        >
          <KeyRound size={14} />
        </button>
      {/if}
      <button
        type="button"
        class="preset-action-icon danger"
        aria-label={tr('Delete {name}', { name: provider.name })}
        title={tr('Delete {name}', { name: provider.name })}
        disabled={deleting}
        onclick={(e) => { e.stopPropagation(); onDelete(provider); }}
      >
        <Trash2 size={14} />
      </button>
    </div>
  </div>
</div>

<style>
  .preset-card-icon {
    position: relative;
    overflow: hidden;
  }
  .preset-card-icon :global(.provider-avatar) {
    width: 100%;
    height: 100%;
    border-radius: inherit;
    display: grid;
    place-items: center;
    position: relative;
    background: transparent !important;
  }
  .preset-card-icon :global(.provider-avatar img) {
    width: 28px;
    height: 28px;
    object-fit: contain;
    position: static;
    padding: 0;
  }
  .preset-card-icon :global(.provider-avatar-fallback) {
    font-size: 16px;
    font-weight: 700;
  }
  .preset-card-actions {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .preset-action-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    border: 1px solid #e2e8f0;
    background: #ffffff;
    color: #64748b;
    cursor: pointer;
    transition: all 0.12s ease;
  }
  .preset-action-icon:hover:not(:disabled) {
    background: #f1f5f9;
    color: #0f172a;
    border-color: #cbd5e1;
  }
  .preset-action-icon.danger:hover:not(:disabled) {
    background: #fef2f2;
    color: #ef4444;
    border-color: #fecaca;
  }
  .preset-action-icon:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .preset-label-badge.warning-label {
    background: #fef2f2;
    color: #dc2626;
  }
  .preset-label-badge.protocol-badge {
    font-size: 11px;
    padding: 1px 7px;
  }

  :global(:root[data-theme='dark']) .preset-action-icon {
    background: rgba(255, 255, 255, 0.04);
    border-color: rgba(255, 255, 255, 0.08);
    color: #94a3b8;
  }
  :global(:root[data-theme='dark']) .preset-action-icon:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.1);
    color: #f1f5f9;
    border-color: rgba(255, 255, 255, 0.15);
  }
  :global(:root[data-theme='dark']) .preset-action-icon.danger:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.18);
    color: #f87171;
    border-color: rgba(239, 68, 68, 0.3);
  }
  :global(:root[data-theme='dark']) .preset-label-badge.warning-label {
    background: rgba(239, 68, 68, 0.16);
    color: #f87171;
  }
</style>
