<script lang="ts">
  import { ArrowDown, ArrowRight, ExternalLink, KeyRound, Link2, Settings2 } from '@lucide/svelte';
  import type { Locale } from '../../lib/i18n';
  import { docsCopy } from './docsCopy';
  import DocsCodeBlock from './DocsCodeBlock.svelte';

  export let locale: Locale;
  $: copy = docsCopy(locale);
</script>

<div class="docs-page docs-article-page docs-integrations-page">
  <header class="docs-article-heading">
    <div class="docs-kicker"><span class="docs-kicker-line"></span>{copy.integrations.eyebrow}</div>
    <h1>{copy.integrations.title}</h1>
    <p>{copy.integrations.intro}</p>
  </header>

  <section class="docs-connection-card">
    <div class="docs-connection-heading"><div class="docs-link-card-icon"><Link2 size={19} /></div><div><span class="docs-kicker">01 · CONNECTION CONTRACT</span><h2>{copy.integrations.connectionTitle}</h2><p>{copy.integrations.connectionBody}</p></div></div>
    <div class="docs-connection-fields">
      {#each copy.integrations.connectionFields as field}<div class="docs-connection-field"><span>{field.label}</span><code>{field.value}</code></div>{/each}
    </div>
    <div class="docs-compatibility-note"><Settings2 size={16} /><span>{copy.integrations.compatibilityNote}</span></div>
  </section>

  <nav class="docs-integrations-index" aria-label={copy.integrations.title}>
    <span class="docs-index-label">JUMP TO</span>
    {#each copy.integrations.guides as guide}<a href={`#${guide.id}`}><span>{guide.title}</span><ArrowDown size={14} /></a>{/each}
  </nav>

  <div class="docs-integration-guides">
    {#each copy.integrations.guides as guide, index}
      <article class="docs-integration-card" id={guide.id}>
        <div class="docs-integration-card-heading"><div class="docs-integration-number">0{index + 2}</div><div><div class="docs-integration-title-row"><h2>{guide.title}</h2><span class="docs-integration-badge">{guide.badge}</span></div><p>{guide.summary}</p></div></div>
        <div class="docs-integration-body">
          <div class="docs-integration-settings">
            <div class="docs-subheading"><KeyRound size={15} /> SETTINGS</div>
            {#each guide.fields as field}<div class="docs-setting-row"><span>{field.label}</span><code>{field.value}</code></div>{/each}
          </div>
          <div class="docs-integration-instructions">
            <div class="docs-subheading">SETUP</div>
            <ol>{#each guide.steps as step}<li>{step}</li>{/each}</ol>
          </div>
        </div>
        {#if guide.code}<DocsCodeBlock sample={guide.code} copyLabel={copy.shell.copy} copiedLabel={copy.shell.copied} copyFailedLabel={copy.shell.copyFailed} />{/if}
        {#if guide.note}<div class="docs-inline-note"><ExternalLink size={15} /><span>{guide.note}</span></div>{/if}
      </article>
    {/each}
  </div>

  <section class="docs-troubleshooting">
    <div class="docs-section-heading"><span class="docs-kicker">LAST CHECK</span><h2>{copy.integrations.troubleshootingTitle}</h2></div>
    <div class="docs-troubleshooting-grid">{#each copy.integrations.troubleshooting as item}<div><h3>{item.title}</h3><p>{item.body}</p></div>{/each}</div>
  </section>
</div>
