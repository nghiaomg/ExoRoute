<script lang="ts">
  import { Activity, ArrowUpRight, Boxes, Clock3, Command, KeyRound, Layers3, Plus, Zap } from '@lucide/svelte';
  import FlyingFishLogo from '../../components/FlyingFishLogo.svelte';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { formatGatewayEndpoint, formatUptime, type Translate } from '../../lib/format';
  import type { Overview, Provider, PublicSettings, GatewayCombo } from '../../lib/types';

  export let overview: Overview;
  export let providers: Provider[];
  export let combos: GatewayCombo[];
  export let settings: PublicSettings;
  export let tr: Translate;
  export let locale: Locale;
  export let liveStreamCount: number;
  export let onCreate: (page: 'providers' | 'combos' | 'api-keys') => void;

  $: enabledProviders = providers.filter((provider) => provider.enabled).length;
  $: gatewayEndpoint = formatGatewayEndpoint(
    settings?.host,
    settings?.port,
    typeof window !== 'undefined' ? window.location?.host : null,
  );
</script>

<section class="overview-hero">
  <div class="overview-hero-left">
    <div class="hero-top-row">
      <div class="hero-status-pill">
        <span class="pulse-dot"></span>
        <span class="hero-status-label">{tr('Gateway Operational')}</span>
        <span class="hero-status-host"><code>{gatewayEndpoint}</code></span>
      </div>
    </div>
    <h1 class="hero-headline">{tr('High-Performance AI Gateway')}</h1>
    <p class="hero-subtitle">{tr('Intelligent routing, multi-protocol conversion (OpenAI, Anthropic, Responses), and automatic low-latency failover.')}</p>
    <div class="hero-actions-row">
      <button class="primary-button compact-btn" onclick={() => onCreate('providers')}><Plus size={14} />{tr('Connect a provider')}</button>
      <button class="secondary-button compact-btn" onclick={() => onCreate('combos')}><Layers3 size={14} />{tr('Create a combo')}</button>
      <button class="secondary-button compact-btn" onclick={() => onCreate('api-keys')}><KeyRound size={14} />{tr('Create key')}</button>
    </div>
  </div>

  <div class="overview-hero-right" aria-label={tr('Client to ExoRoute to provider flow')}>
    <div class="pipeline-card">
      <div class="pipeline-header">
        <div class="pipeline-title-group">
          <span class="pipeline-pulse-dot" class:pulse-online={liveStreamCount > 0}></span>
          <span class="pipeline-title">{tr('Active Pipeline')}</span>
        </div>
        <span class="pipeline-badge" class:has-live={liveStreamCount > 0}>
          <Zap size={11} />
          {#if overview.provider_count === 1}
            1 {tr('Provider').toUpperCase()}
          {:else}
            {overview.provider_count} {tr('PROVIDERS')}
          {/if}
        </span>
      </div>
      <div class="pipeline-diagram">
        <div
          class="pipeline-node client-node"
          title={tr('Client SDK')}
        >
          <div class="node-icon"><Command size={16} /></div>
          <div class="node-text">
            <strong>{tr('CLIENT')}</strong>
            <small>OpenAI / SDK</small>
          </div>
        </div>

        <div class="pipeline-stream stream-inbound" title={tr('Inbound request stream from client SDK')}>
          <div class="stream-line"></div>
          <div class="stream-particle"></div>
          <span class="stream-label">{tr('Inbound')}</span>
        </div>

        <div
          class="pipeline-node core-node"
          title={tr('Protocol translation and intelligent routing core')}
        >
          <div class="core-logo-badge">
            <FlyingFishLogo size={18} variant="white" />
          </div>
          <div class="node-text">
            <strong>{tr('EXOROUTE')}</strong>
            <small>{tr('Protocol Core')}</small>
          </div>
        </div>

        <div class="pipeline-stream stream-outbound" title={tr('Multi-protocol conversion and upstream dispatch')}>
          <div class="stream-line"></div>
          <div class="stream-particle particle-mint"></div>
          <span class="stream-label">{tr('Translate')}</span>
        </div>

        <div
          class="pipeline-node upstream-node"
          title={tr('Connected upstream providers')}
        >
          <div class="upstream-chips">
            <div class="upstream-mini-chip stream-count-chip">{liveStreamCount}</div>
          </div>
          <div class="node-text">
            <strong>{tr('UPSTREAM')}</strong>
            <small>
              <span class="live-dot" class:is-online={liveStreamCount > 0}></span>
              {liveStreamCount} {tr('LIVE')}
            </small>
          </div>
        </div>
      </div>
    </div>
  </div>
</section>

<section class="metric-grid">
  <article class="metric-card">
    <div class="metric-card-inner">
      <span class="metric-icon mint"><Boxes size={18} /></span>
      <div class="metric-info-col">
        <div class="metric-top">
          <span class="metric-label">{tr('PROVIDERS')}</span>
          <span class="metric-trend mint-trend"><ArrowUpRight size={12} /> {tr('LIVE')}</span>
        </div>
        <div class="metric-bottom">
          <div class="metric-value">{overview.provider_count}</div>
          <div class="metric-foot">
            <span class="metric-pill success-pill"><span class="metric-dot mint-dot"></span>{enabledProviders} {tr('enabled')}</span>
            {#if providers.length - enabledProviders > 0}
              <span class="metric-pill neutral-pill">{providers.length - enabledProviders} {tr('disabled')}</span>
            {/if}
          </div>
        </div>
      </div>
    </div>
    <div class="metric-accent mint-accent"></div>
  </article>

  <article class="metric-card">
    <div class="metric-card-inner">
      <span class="metric-icon violet"><Layers3 size={18} /></span>
      <div class="metric-info-col">
        <div class="metric-top">
          <span class="metric-label">{tr('ACTIVE COMBOS')}</span>
          <span class="metric-trend violet-trend"><ArrowUpRight size={12} /> {tr('LIVE')}</span>
        </div>
        <div class="metric-bottom">
          <div class="metric-value">{combos.length}</div>
          <div class="metric-foot">
            <span class="metric-foot-desc">{combos.reduce((count, combo) => count + combo.targets.length, 0)} {tr('Active targets')}</span>
          </div>
        </div>
      </div>
    </div>
    <div class="metric-accent violet-accent"></div>
  </article>

  <article class="metric-card">
    <div class="metric-card-inner">
      <span class="metric-icon blue"><Activity size={18} /></span>
      <div class="metric-info-col">
        <div class="metric-top">
          <span class="metric-label">{tr('TOTAL REQUESTS')}</span>
          <span class="metric-trend blue-trend"><ArrowUpRight size={12} /> {tr('LIVE')}</span>
        </div>
        <div class="metric-bottom">
          <div class="metric-value">{overview.request_count.toLocaleString(getIntlLocale(locale))}</div>
          <div class="metric-foot">
            <span class="metric-foot-desc">{tr('Since gateway startup')}</span>
          </div>
        </div>
      </div>
    </div>
    <div class="metric-accent blue-accent"></div>
  </article>

  <article class="metric-card">
    <div class="metric-card-inner">
      <span class="metric-icon amber"><Clock3 size={18} /></span>
      <div class="metric-info-col">
        <div class="metric-top">
          <span class="metric-label">{tr('UPTIME')}</span>
          <span class="metric-trend amber-trend"><span class="metric-dot mint-dot"></span> {tr('ONLINE')}</span>
        </div>
        <div class="metric-bottom">
          <div class="metric-value metric-uptime">{formatUptime(overview.uptime_seconds, tr)}</div>
          <div class="metric-foot">
            <span class="metric-foot-desc">{tr('Since last restart')}</span>
          </div>
        </div>
      </div>
    </div>
    <div class="metric-accent amber-accent"></div>
  </article>
</section>
