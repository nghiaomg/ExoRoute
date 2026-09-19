<script lang="ts">
  import { ArrowRight, Boxes, Cpu, Layers3 } from '@lucide/svelte';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Translate } from '../../lib/format';

  export let tr: Translate;
  export let onCreateRequest: (page: DashboardPage) => void;
  export let onNavigate: (page: DashboardPage) => void;
</script>

<div class="quick-action-strip" aria-label={tr('Quick setup')}>
  <div class="quick-strip-label">
    <span class="quick-strip-dot"></span>
    <span class="quick-strip-title">{tr('Quick setup')}</span>
  </div>
  <div class="quick-strip-items">
    <button type="button" class="quick-strip-btn" onclick={() => onCreateRequest('providers')}>
      <span class="quick-step-num">01</span>
      <span class="quick-step-icon mint-icon"><Boxes size={13} /></span>
      <span class="quick-step-text">{tr('Connect a provider')}</span>
      <ArrowRight size={12} class="quick-step-arrow" />
    </button>
    <button type="button" class="quick-strip-btn" onclick={() => onNavigate('providers')}>
      <span class="quick-step-num">02</span>
      <span class="quick-step-icon blue-icon"><Cpu size={13} /></span>
      <span class="quick-step-text">{tr('Manage provider models')}</span>
      <ArrowRight size={12} class="quick-step-arrow" />
    </button>
    <button type="button" class="quick-strip-btn" onclick={() => onCreateRequest('combos')}>
      <span class="quick-step-num">03</span>
      <span class="quick-step-icon violet-icon"><Layers3 size={13} /></span>
      <span class="quick-step-text">{tr('Create a combo')}</span>
      <ArrowRight size={12} class="quick-step-arrow" />
    </button>
  </div>
</div>

