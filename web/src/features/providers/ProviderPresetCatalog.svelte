<script lang="ts">
  import { Bot, Boxes, LoaderCircle, Sliders, Terminal, Zap } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';
  import type { Provider, ProviderCategory, ProviderLabel, ProviderPreset } from '../../lib/types';
  import { providerLogo } from './provider-logo';

  type CategoryDefinition = {
    category: ProviderCategory;
    titleKey: string;
    descriptionKey: string;
  };

  const categories: CategoryDefinition[] = [
    {
      category: 'cloud_api',
      titleKey: 'Provider category: Cloud API',
      descriptionKey: 'Official APIs from the provider itself.',
    },
    {
      category: 'gateway',
      titleKey: 'Provider category: Gateway',
      descriptionKey: 'Hosted gateways that expose multiple models.',
    },
    {
      category: 'oauth',
      titleKey: 'Provider category: OAuth',
      descriptionKey: 'Providers authenticated through OAuth.',
    },
  ];

  export let presets: ProviderPreset[];
  export let providers: Provider[];
  export let tr: Translate;
  export let openingPresetId = '';
  export let onSelect: (preset: ProviderPreset, provider: Provider | null) => void;

  function selectPreset(preset: ProviderPreset): void {
    if (openingPresetId) return;
    const provider = providers.find((item) => item.adapter_id === preset.adapter_id) ?? null;
    onSelect(preset, provider);
  }

  function labelTranslationKey(label: ProviderLabel): string {
    return label === 'free' ? 'Free' : 'Free Tier';
  }
</script>

{#each categories as category (category.category)}
  {@const categoryPresets = presets.filter((preset) => preset.category === category.category)}
  {#if categoryPresets.length > 0}
    <section class="provider-category-section preset-category-group">
      <div class="category-section-header">
        <div>
          <h3 class="category-title">{tr(category.titleKey)}</h3>
          <p class="category-subtitle">{tr(category.descriptionKey)}</p>
        </div>
        <span class="category-count-badge">{categoryPresets.length} {tr('available')}</span>
      </div>

      <div class="preset-cards-grid">
          {#each categoryPresets as preset (preset.id)}
            {@const connectedProvider = providers.find((provider) => provider.adapter_id === preset.adapter_id) ?? null}
            {@const isConnected = Boolean(connectedProvider)}
            {@const logoSrc = providerLogo(preset.default_logo_url, preset.adapter_id, preset.id, preset.name)}
            <div
              class="preset-card"
              class:is-connected={isConnected}
              aria-disabled={Boolean(openingPresetId)}
              aria-busy={openingPresetId === preset.id}
              role="button"
              tabindex={openingPresetId ? -1 : 0}
              onkeydown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault();
                  selectPreset(preset);
                }
              }}
              onclick={() => selectPreset(preset)}
            >
              <div class="preset-card-top">
                <div
                  class="preset-card-icon"
                  class:kilo={preset.id === 'kilo_gateway'}
                  class:codex={preset.id === 'openai_codex'}
                  class:command={preset.id === 'command_code'}
                >
                  {#if logoSrc}
                    <img src={logoSrc} alt="" loading="lazy" />
                  {:else if preset.id === 'kilo_gateway'}
                    <Zap size={22} />
                  {:else if preset.id === 'openai_codex'}
                    <Bot size={22} />
                  {:else if preset.id === 'command_code'}
                    <Terminal size={22} />
                  {:else}
                    <Boxes size={22} />
                  {/if}
                </div>
                <span class="preset-badge" class:connected-badge={isConnected}>
                  {#if isConnected}
                    {tr('Connected')}
                  {:else if preset.capabilities.oauth_accounts && preset.capabilities.api_keys}
                    {tr('OAuth + API key')}
                  {:else if preset.capabilities.oauth_accounts}
                    {tr('OAuth 2.0')}
                  {:else if preset.supported_auth_types.includes('header')}
                    {tr('Header / Key')}
                  {:else}
                    {tr('API Key')}
                  {/if}
                </span>
              </div>

              <div class="preset-card-body">
                <h4>{tr(preset.name)}</h4>
                <p>{tr(preset.description)}</p>
                {#if preset.labels.length > 0}
                  <div class="preset-labels" aria-label={tr('Provider labels')}>
                    {#each preset.labels as label (label)}
                      <span
                        class="preset-label-badge"
                        class:free-label={label === 'free'}
                        class:free-tier-label={label === 'free_tier'}
                      >{tr(labelTranslationKey(label))}</span>
                    {/each}
                  </div>
                {/if}
              </div>

              <div class="preset-card-footer">
                {#if openingPresetId === preset.id}
                  <span class="preset-detail-btn" aria-live="polite">
                    <LoaderCircle size={13} class="spin" />
                    <span>{tr('Opening provider…')}</span>
                  </span>
                {:else}
                  <span class="preset-detail-btn">
                    <Sliders size={13} />
                    <span>{tr('Provider details')}</span>
                  </span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
    </section>
  {/if}
{/each}
