<script lang="ts">
  import { AlertTriangle, Database, KeyRound, ShieldCheck } from '@lucide/svelte';
  import type { Locale } from '../../lib/i18n';
  import { docsCopy } from './docsCopy';
  import DocsCodeBlock from './DocsCodeBlock.svelte';

  export let locale: Locale;
  $: copy = docsCopy(locale);
</script>

<div class="docs-page docs-article-page docs-reference-page">
  <header class="docs-article-heading">
    <div class="docs-kicker"><span class="docs-kicker-line"></span>{copy.reference.eyebrow}</div>
    <h1>{copy.reference.title}</h1>
    <p>{copy.reference.intro}</p>
  </header>

  <section class="docs-reference-section">
    <div class="docs-section-heading"><span class="docs-kicker">01 · HTTP SURFACE</span><h2>{copy.reference.endpointTitle}</h2></div>
    <div class="docs-endpoint-table-wrap">
      <table class="docs-endpoint-table">
        <thead><tr>{#each copy.reference.endpointHeaders as header}<th>{header}</th>{/each}</tr></thead>
        <tbody>{#each copy.reference.endpoints as endpoint}<tr><td><span class:docs-method-post={endpoint.method === 'POST'} class:docs-method-get={endpoint.method === 'GET'} class:docs-method-delete={endpoint.method === 'DELETE'}>{endpoint.method}</span></td><td><code>{endpoint.path}</code></td><td>{endpoint.protocol}</td><td>{endpoint.description}</td></tr>{/each}</tbody>
      </table>
    </div>
  </section>

  <section class="docs-reference-two-column">
    <article class="docs-reference-card"><div class="docs-reference-icon"><KeyRound size={18} /></div><span class="docs-kicker">02 · AUTH</span><h2>{copy.reference.authTitle}</h2><p>{copy.reference.authBody}</p><DocsCodeBlock sample={copy.reference.authCode} copyLabel={copy.shell.copy} copiedLabel={copy.shell.copied} copyFailedLabel={copy.shell.copyFailed} /></article>
    <article class="docs-reference-card"><div class="docs-reference-icon"><Database size={18} /></div><span class="docs-kicker">03 · MODELS</span><h2>{copy.reference.modelTitle}</h2><p>{copy.reference.modelBody}</p><DocsCodeBlock sample={copy.reference.modelCode} copyLabel={copy.shell.copy} copiedLabel={copy.shell.copied} copyFailedLabel={copy.shell.copyFailed} /></article>
  </section>

  <section class="docs-reference-section">
    <div class="docs-section-heading"><span class="docs-kicker">04 · PROCESS ENVIRONMENT</span><h2>{copy.reference.environmentTitle}</h2><p>{copy.reference.environmentBody}</p></div>
    <div class="docs-environment-table-wrap"><table class="docs-environment-table"><thead><tr><th>Variable</th><th>Typical value</th><th>Purpose</th></tr></thead><tbody>{#each copy.reference.environmentRows as row}<tr><td><code>{row.name}</code></td><td><code>{row.value}</code></td><td>{row.description}</td></tr>{/each}</tbody></table></div>
  </section>

  <section class="docs-safety-section">
    <div class="docs-safety-heading"><div class="docs-security-icon"><ShieldCheck size={18} /></div><div><span class="docs-kicker">05 · OPERATIONS</span><h2>{copy.reference.safetyTitle}</h2></div></div>
    <ul class="docs-safety-list">{#each copy.reference.safetyItems as item}<li><AlertTriangle size={15} /><span>{item}</span></li>{/each}</ul>
  </section>
</div>
