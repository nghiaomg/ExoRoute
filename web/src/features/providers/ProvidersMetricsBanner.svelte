<script lang="ts">
  import { Cpu, KeyRound, Layers, Server } from '@lucide/svelte';
  import { type Translate } from '../../lib/format';
  import type { Provider, ProviderPreset } from '../../lib/types';

  export let providers: Provider[];
  export let presets: ProviderPreset[];
  export let tr: Translate;

  $: totalCredentials = providers.reduce(
    (sum, p) => sum + (p.api_key_count ?? (p.api_key ? 1 : 0)),
    0
  );
  $: totalModels = providers.reduce((sum, p) => sum + (p.model_count ?? 0), 0);
  $: availablePresetCount = presets.filter(
    (p) => p.id !== 'custom' && p.adapter_id !== 'generic'
  ).length + 1;
</script>

<section class="providers-metrics-banner" aria-label={tr('Quick Overview')}>
  <div class="metric-chip">
    <div class="metric-chip-icon providers-icon"><Server size={18} /></div>
    <div class="metric-chip-info">
      <strong>{providers.length}</strong>
      <span>{tr('Connected Providers')}</span>
    </div>
  </div>
  <div class="metric-chip">
    <div class="metric-chip-icon credentials-icon"><KeyRound size={18} /></div>
    <div class="metric-chip-info">
      <strong>{totalCredentials}</strong>
      <span>{tr('Total Credentials')}</span>
    </div>
  </div>
  <div class="metric-chip">
    <div class="metric-chip-icon models-icon"><Cpu size={18} /></div>
    <div class="metric-chip-info">
      <strong>{totalModels}</strong>
      <span>{tr('Saved Models')}</span>
    </div>
  </div>
  <div class="metric-chip">
    <div class="metric-chip-icon presets-icon"><Layers size={18} /></div>
    <div class="metric-chip-info">
      <strong>{availablePresetCount}</strong>
      <span>{tr('Available Presets')}</span>
    </div>
  </div>
</section>
