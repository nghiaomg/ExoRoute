<script lang="ts">
  /**
   * Shared dashboard statistic card: icon + headline number, title, one-line
   * trend, and an optional `details-link` action. Tailwind utilities reference
   * the theme bridge in styles/tailwind.css; `!` marks utilities that must
   * out-rank unlayered legacy rules (.dashboard-app button color reset, .icon
   * sizing). The KPI uses the approved number-accent hover treatment.
   */
  interface Props {
    /** Sprite id for the icon, with or without the leading `#`. */
    icon?: string;
    /** Localized category shown above the value. */
    eyebrow?: string;
    /** Headline value, already formatted by the caller. */
    value?: string | number;
    /** Card heading; only needed when rendering a real card. */
    title?: string;
    /** One muted line under the title; omit to render no trend row. */
    trend?: string;
    /** When set, renders the details-link button with this label. */
    actionLabel?: string;
    onAction?: () => void;
    /** Render the empty loading placeholder instead of content. */
    skeleton?: boolean;
    skeletonHeight?: number;
  }

  let {
    icon = '',
    eyebrow = '',
    value = '',
    title = '',
    trend,
    actionLabel,
    onAction,
    skeleton = false,
    skeletonHeight = 132,
  }: Props = $props();

  const iconHref = $derived(icon.startsWith('#') ? icon : `#${icon}`);

  const focusRing = 'focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring';
  const cardBase =
    'stats-card group/stat relative flex min-h-[152px] min-w-0 flex-col rounded-[17px] border border-line bg-surface p-[18px] ' +
    'max-nav:min-h-[116px] max-nav:rounded-[13px] max-nav:p-4';
  const topClass =
    'mb-5 flex min-w-0 items-center justify-between gap-2';
  const iconClass =
    'grid size-5 shrink-0 place-items-center text-muted';
  const numberClass =
    'stat-value overflow-hidden text-ellipsis text-ink font-bold tabular-nums tracking-[-0.035em] leading-none origin-right ' +
    'transition-colors duration-200 motion-reduce:transition-none';
  const titleClass =
    'stat-title m-0 mt-2 mb-1 overflow-hidden text-ellipsis text-ink font-semibold tracking-[-0.01em] whitespace-nowrap @max-[920px]:mt-2 max-nav:whitespace-normal max-nav:overflow-visible';
  const trendClass =
    'stat-meta block min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-muted leading-[1.45]';
  const actionClass = `stat-action stat-meta inline-flex w-fit items-center gap-1 mt-1.5 border-0 bg-transparent p-0 text-ink font-medium text-left cursor-pointer ${focusRing}`;
</script>

{#if skeleton}
  <div class={cardBase} style="min-height:{skeletonHeight}px" aria-hidden="true"></div>
{:else}
  <article class={cardBase}>
    <div class={topClass}>
      <span class="stat-eyebrow">{eyebrow}</span>
      <span class={iconClass} aria-hidden="true"><svg class="icon size-full!"><use href={iconHref} /></svg></span>
    </div>
    <span class={numberClass} data-stat-value>{value}</span>
    <h2 class={titleClass}>{title}</h2>
    {#if trend !== undefined}
      <div class={trendClass}>{trend}</div>
    {/if}
    {#if actionLabel !== undefined}
      <button type="button" onclick={onAction} class={actionClass}>
        {actionLabel}
        <svg class="icon size-[11px]!"><use href="#i-arrow-right" /></svg>
      </button>
    {/if}
  </article>
{/if}

<style>
  .stat-value { font-size: var(--type-kpi, 2rem); }
  .stat-eyebrow { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted); font-family: var(--font-mono, monospace); font-size: 10px; letter-spacing: .12em; text-transform: uppercase; }
  .stat-title { font-size: var(--type-label, .875rem); }
  .stat-meta { font-size: var(--type-meta, .75rem); }
  .stat-action:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
  .stats-card:hover .stat-value { color: var(--accent); }
  .stats-card { transition: border-color .22s ease, background-color .22s ease; }
  .stats-card:hover {
    border-color: transparent;
    background: linear-gradient(var(--surface), var(--surface)) padding-box,
      linear-gradient(120deg, var(--accent), #fb923c, #fbbf24, #84cc16, var(--accent)) border-box;
  }
  @media (prefers-reduced-motion: reduce) {
    .stats-card { transition: none; }
  }
</style>
