<script lang="ts">
  import { ArrowRight, Boxes, Layers3, Plus } from '@lucide/svelte';
  import { type Translate } from '../../lib/format';
import { labelProtocol, labelStrategy } from '../../lib/labels';
  import type { DashboardPage } from '../../lib/navigation';
  import type { GatewayCombo, Provider } from '../../lib/types';
  import ProviderAvatar from '../providers/ProviderAvatar.svelte';

  export let combos: GatewayCombo[];
  export let providers: Provider[];
  export let tr: Translate;
  export let onNavigate: (page: DashboardPage) => void;
  export let onCreateRequest: (page: DashboardPage) => void;
</script>

<section class="overview-side-col">
  <div class="section-card side-card">
    <div class="section-card-header compact-header">
      <div class="header-with-icon"><span class="panel-icon violet-panel"><Layers3 size={17} /></span><div><h3 class="side-card-title">{tr('Active combos')}</h3><p class="side-card-caption">{tr('Active combos & fallback targets')}</p></div></div>
      <button class="text-button compact-btn" onclick={() => onNavigate('combos')}>{tr('View all')} <ArrowRight size={13} /></button>
    </div>
    <div class="side-card-body">
      {#if combos.length}
        <div class="combos-mini-list">
          {#each combos.slice(0, 3) as combo}
            <div class="combo-mini-item">
              <div class="combo-mini-top"><strong>{combo.name}</strong><span class="strategy-pill">{labelStrategy(combo.strategy, tr)}</span></div>
              <div class="combo-mini-targets">
                {#each combo.targets.slice(0, 2) as target, index}<span class="mini-target-chip"><span class="target-idx">{index + 1}</span><code>{target.model}</code></span>{/each}
                {#if combo.targets.length > 2}<span class="mini-target-more">+{combo.targets.length - 2}</span>{/if}
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="side-empty-state"><p>{tr('No combos configured')}</p><button class="secondary-button compact" onclick={() => onCreateRequest('combos')}><Plus size={13} />{tr('Create combo')}</button></div>
      {/if}
    </div>
  </div>

  <div class="section-card side-card">
    <div class="section-card-header compact-header">
      <div class="header-with-icon"><span class="panel-icon amber"><Boxes size={17} /></span><div><h3 class="side-card-title">{tr('Providers')}</h3><p class="side-card-caption">{tr('Connected Providers')}</p></div></div>
      <button class="text-button compact-btn" onclick={() => onNavigate('providers')}>{tr('View all')} <ArrowRight size={13} /></button>
    </div>
    <div class="side-card-body">
      {#if providers.length}
        <div class="providers-mini-list">
          {#each providers.slice(0, 3) as provider}
            <div class="provider-mini-row"><ProviderAvatar name={provider.name} logoUrl={provider.logo_url} adapterId={provider.adapter_id} providerId={provider.id} className="provider-mini-avatar" /><div class="provider-mini-info"><strong>{provider.name}</strong><small>{labelProtocol(provider.preferred_protocol, tr)} · {provider.model_count ?? 0} {tr('saved models')}</small></div><span class="state-dot-wrap" title={tr(provider.enabled ? 'Enabled' : 'Disabled')}><span class="state-dot" class:active={provider.enabled}></span></span></div>
          {/each}
        </div>
      {:else}
        <div class="side-empty-state"><p>{tr('No providers connected')}</p><button class="secondary-button compact" onclick={() => onCreateRequest('providers')}><Plus size={13} />{tr('Connect a provider')}</button></div>
      {/if}
    </div>
  </div>
</section>
