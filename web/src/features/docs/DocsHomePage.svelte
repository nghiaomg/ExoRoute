<script lang="ts">
  import { ArrowRight, Boxes, CheckCircle2, LockKeyhole, Route, Server } from '@lucide/svelte';
  import type { Locale } from '../../lib/i18n';
  import type { DocsPage } from '../../lib/navigation';
  import { docsCopy } from './docsCopy';

  export let locale: Locale;
  export let onNavigate: (page: DocsPage) => void;

  $: copy = docsCopy(locale);

  const cardIcons = [Route, Boxes, Server];

  function pathFor(page: DocsPage): string {
    return page === 'docs' ? '/docs' : `/${page.replace('docs-', 'docs/')}`;
  }

  function openPage(event: MouseEvent, page: DocsPage): void {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    onNavigate(page);
  }
</script>

<div class="docs-page docs-home-page">
  <section class="docs-hero">
    <div class="docs-hero-copy">
      <div class="docs-kicker"><span class="docs-kicker-line"></span>{copy.home.eyebrow}</div>
      <h1>{copy.home.title}</h1>
      <p class="docs-hero-intro">{copy.home.intro}</p>
      <div class="docs-hero-actions">
        <a class="docs-primary-button" href="/docs/quickstart" onclick={(event) => openPage(event, 'docs-quickstart')}>{copy.home.primaryCta}<ArrowRight size={16} /></a>
        <a class="docs-secondary-button" href="/docs/integrations" onclick={(event) => openPage(event, 'docs-integrations')}>{copy.home.secondaryCta}</a>
      </div>
      <div class="docs-badge-row">
        {#each copy.home.badges as badge}<span><CheckCircle2 size={14} />{badge}</span>{/each}
      </div>
    </div>
    <div class="docs-hero-visual" aria-label={copy.home.architectureTitle}>
      <div class="docs-visual-glow"></div>
      <div class="docs-orbit docs-orbit-one"></div>
      <div class="docs-orbit docs-orbit-two"></div>
      <div class="docs-visual-center"><span class="docs-visual-center-mark">ER</span><strong>ExoRoute</strong><small>{copy.shell.localFirst}</small></div>
      <div class="docs-visual-node docs-node-client"><span><Route size={16} /></span><small>{copy.home.flow[0].label}</small></div>
      <div class="docs-visual-node docs-node-provider"><span><Server size={16} /></span><small>{copy.home.flow[2].label}</small></div>
      <div class="docs-visual-node docs-node-key"><span><LockKeyhole size={15} /></span><small>API key</small></div>
    </div>
  </section>

  <section class="docs-card-grid" aria-label={copy.shell.documentation}>
    {#each copy.home.cards as card, index}
      {@const Icon = cardIcons[index]}
      <a class="docs-link-card" href={pathFor(card.page)} onclick={(event) => openPage(event, card.page)}>
        <div class="docs-link-card-icon"><Icon size={20} strokeWidth={1.8} /></div>
        <div class="docs-link-card-copy"><span class="docs-card-arrow"><ArrowRight size={16} /></span><h2>{card.title}</h2><p>{card.description}</p><strong>{card.cta} <ArrowRight size={14} /></strong></div>
      </a>
    {/each}
  </section>

  <section class="docs-section docs-architecture-section">
    <div class="docs-section-heading"><span class="docs-kicker">{copy.home.eyebrow}</span><h2>{copy.home.architectureTitle}</h2><p>{copy.home.architectureIntro}</p></div>
    <div class="docs-flow">
      {#each copy.home.flow as item, index}
        <div class="docs-flow-item"><span class="docs-flow-number">0{index + 1}</span><div><strong>{item.label}</strong><p>{item.description}</p></div>{#if index < copy.home.flow.length - 1}<ArrowRight class="docs-flow-arrow" size={19} />{/if}</div>
      {/each}
    </div>
  </section>

  <section class="docs-security-callout">
    <div class="docs-security-icon"><LockKeyhole size={19} /></div>
    <div><strong>{copy.home.securityTitle}</strong><p>{copy.home.securityBody}</p></div>
  </section>
</div>
