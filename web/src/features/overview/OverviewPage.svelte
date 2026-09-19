<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import GatewayError from '../../components/GatewayError.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import { api } from '../../lib/api';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { GatewayCombo, Overview, Provider, PublicSettings, RequestLog } from '../../lib/types';
  import OverviewHighlights from './OverviewHighlights.svelte';
  import OverviewQuickSetup from './OverviewQuickSetup.svelte';
  import OverviewRoutingSummary from './OverviewRoutingSummary.svelte';
  import OverviewTraffic from './OverviewTraffic.svelte';

  export let tr: Translate;
  export let locale: Locale;
  export let onNavigate: (page: DashboardPage) => void;
  export let onCreateRequest: (page: DashboardPage) => void;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  export let onProviderCountChange: (count: number) => void;
  export let onGatewayAddressChange: (host: unknown, port: unknown) => void;

  let overview: Overview | null = null;
  let providers: Provider[] = [];
  let combos: GatewayCombo[] = [];
  let requests: RequestLog[] = [];
  let settings: PublicSettings = {};
  let loading = true;
  let errorMessage = '';
  let generation = 0;
  let liveStreamCount = 0;

  async function load(): Promise<void> {
    const requestGeneration = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const [summary, providerRows, comboRows, requestRows, settingsRows] = await Promise.all([
        api.overview(), api.providers(), api.combos(), api.requests(), api.settings(),
      ]);
      if (requestGeneration !== generation) return;
      overview = summary;
      providers = providerRows;
      combos = comboRows;
      requests = requestRows.requests;
      settings = settingsRows;
      onGatewayAddressChange(settingsRows.host, settingsRows.port);
      onProviderCountChange(providerRows.length);
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation) return;
      errorMessage = localizedError(error, 'Something went wrong while loading this page.', tr);
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) loading = false;
    }
  }

  onMount(() => { void load(); });
  onDestroy(() => {
    generation += 1;
  });
</script>

<PageHeading title={tr('Overview')} subtitle={tr('A clear view of your gateway, at a glance.')} {tr} kicker="EXOROUTE CONTROL PLANE" />
{#if errorMessage}
  <GatewayError message={errorMessage} {tr} onRetry={load} />
{:else if loading || !overview}
  <section class="loading-grid" aria-label={tr('Loading dashboard')}><div class="skeleton hero-skeleton"></div><div class="skeleton"></div><div class="skeleton"></div><div class="skeleton"></div></section>
{:else}
  <div class="overview-view">
    <OverviewHighlights {overview} {providers} {combos} {settings} {tr} {locale} {liveStreamCount} onCreate={onCreateRequest} />
    <OverviewQuickSetup {tr} {onCreateRequest} {onNavigate} />
    <div class="overview-dual-grid">
      <OverviewTraffic
        {requests}
        {tr}
        {locale}
        {onNavigate}
        onLiveCountChange={(count) => { liveStreamCount = count; }}
        onRequestFinished={() => {
          if (overview) {
            overview = { ...overview, request_count: overview.request_count + 1 };
          }
        }}
      />
      <OverviewRoutingSummary {combos} {providers} {tr} {onNavigate} onCreateRequest={onCreateRequest} />
    </div>
  </div>
{/if}
