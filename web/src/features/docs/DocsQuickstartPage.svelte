<script lang="ts">
  import { ArrowRight, Check, ShieldCheck } from '@lucide/svelte';
  import type { Locale } from '../../lib/i18n';
  import { docsCopy } from './docsCopy';
  import DocsCodeBlock from './DocsCodeBlock.svelte';

  export let locale: Locale;
  $: copy = docsCopy(locale);
</script>

<div class="docs-page docs-article-page">
  <header class="docs-article-heading">
    <div class="docs-kicker"><span class="docs-kicker-line"></span>{copy.quickstart.eyebrow}</div>
    <h1>{copy.quickstart.title}</h1>
    <p>{copy.quickstart.intro}</p>
  </header>

  <div class="docs-steps">
    {#each copy.quickstart.steps as step}
      <article class="docs-step-card">
        <div class="docs-step-number">{step.number}</div>
        <div class="docs-step-content"><h2>{step.title}</h2><p>{step.body}</p>{#if step.code}<DocsCodeBlock sample={step.code} copyLabel={copy.shell.copy} copiedLabel={copy.shell.copied} copyFailedLabel={copy.shell.copyFailed} />{/if}{#if step.note}<div class="docs-inline-note"><ShieldCheck size={15} /><span>{step.note}</span></div>{/if}</div>
      </article>
    {/each}
  </div>

  <section class="docs-article-section">
    <div class="docs-section-heading"><span class="docs-kicker">02 · VERIFY THE PATH</span><h2>{copy.quickstart.requestTitle}</h2><p>{copy.quickstart.requestBody}</p></div>
    <DocsCodeBlock sample={copy.quickstart.request} copyLabel={copy.shell.copy} copiedLabel={copy.shell.copied} copyFailedLabel={copy.shell.copyFailed} />
    <DocsCodeBlock sample={copy.quickstart.powershell} copyLabel={copy.shell.copy} copiedLabel={copy.shell.copied} copyFailedLabel={copy.shell.copyFailed} />
  </section>

  <section class="docs-article-section docs-checklist-section">
    <div class="docs-section-heading"><span class="docs-kicker">03 · CHECK BEFORE YOU SCALE</span><h2>{copy.quickstart.checklistTitle}</h2></div>
    <ul class="docs-checklist">
      {#each copy.quickstart.checklist as item}<li><span><Check size={14} /></span>{item}</li>{/each}
    </ul>
  </section>

  <section class="docs-next-card">
    <div><span class="docs-kicker">NEXT</span><h2>{copy.quickstart.nextTitle}</h2><p>{copy.quickstart.nextBody}</p></div>
    <a class="docs-primary-button" href="/docs/integrations">{copy.shell.documentation}<ArrowRight size={16} /></a>
  </section>
</div>
