<script lang="ts">
  /**
   * Shared dashboard statistic card — variant 06 from
   * sketch/components/StatsCardVariants.html: mono uppercase kicker + right
   * badge on top, 32px KPI number, title and one muted subtitle line below.
   * Hover swaps the border for the accent gradient wash and tints the KPI.
   * The previous icon circle and "details-link" action are removed by design.
   * Colors resolve through the theme bridge in styles/tailwind.css.
   */
  interface Props {
    /** Localized category shown above the value (distinct from the title). */
    eyebrow?: string;
    /** 1-based card position on the page — renders the sketch's `02 / NETWORK` kicker. */
    index?: number;
    /** Short right-hand badge text; pair with `badgeTone`. */
    badge?: string;
    /** Badge treatment: green pill, gray chip, green text, muted text, or a status dot. */
    badgeTone?: 'pill' | 'chip' | 'success' | 'text' | 'dot';
    /** Headline value, already formatted by the caller. */
    value?: string | number;
    /** Small muted unit suffix rendered next to the KPI (e.g. 'ms'). */
    unit?: string | null;
    /** Card heading; only needed when rendering a real card. */
    title?: string;
    /** One muted line under the title; omit to render no subtitle row. */
    trend?: string;
    /** Render the empty loading placeholder instead of content. */
    skeleton?: boolean;
    skeletonHeight?: number;
  }

  let {
    eyebrow = '',
    index,
    badge,
    badgeTone = 'text',
    value = '',
    unit,
    title = '',
    trend,
    skeleton = false,
    skeletonHeight = 152,
  }: Props = $props();

  const cardClass =
    'min-w-0 min-h-[152px] rounded-[17px] border border-line bg-surface p-[18px] ' +
    'transition-[transform,box-shadow,border-color,background-color] duration-200 ' +
    'hover:border-transparent! hover:bg-[linear-gradient(var(--surface),var(--surface))_padding-box,linear-gradient(120deg,var(--accent)_0%,var(--accent-glow)_35%,var(--accent-soft)_70%,var(--accent)_100%)_border-box]';
  const kickerClass =
    'mb-5 flex min-w-0 items-center justify-between gap-2 font-mono text-[10px] uppercase tracking-[0.16em] text-muted';
  const kpiClass =
    'block overflow-hidden text-ellipsis text-[32px] font-bold leading-none tracking-[-0.055em] text-ink transition-colors duration-200 group-hover/stat:text-accent!';
</script>

{#if skeleton}
  <div class={cardClass} style="min-height:{skeletonHeight}px" aria-hidden="true"></div>
{:else}
  <div class="group/stat {cardClass}">
    <div class={kickerClass}>
      <span class="stat-eyebrow min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">{index !== undefined ? `${String(index).padStart(2, '0')} / ${eyebrow}` : eyebrow}</span>
      {#if badge}
        {#if badgeTone === 'pill'}
          <span class="shrink-0 rounded-full bg-db-success/10 px-2 py-1 tracking-normal text-db-success">{badge}</span>
        {:else if badgeTone === 'chip'}
          <span class="shrink-0 rounded-md bg-db-tint-strong px-2 py-1 tracking-normal text-muted">{badge}</span>
        {:else if badgeTone === 'success'}
          <span class="shrink-0 tracking-normal text-db-success">{badge}</span>
        {:else if badgeTone === 'dot'}
          <span class="size-2 shrink-0 rounded-full bg-db-success ring-4 ring-db-success/10" title={badge}></span>
        {:else}
          <span class="shrink-0">{badge}</span>
        {/if}
      {/if}
    </div>
    <span class={kpiClass}>{value}{#if unit}<span class="ms-1 text-xl font-semibold text-muted">{unit}</span>{/if}</span>
    <h2 class="stat-title m-0 mt-2 overflow-hidden text-ellipsis whitespace-nowrap text-sm font-semibold text-ink">{title}</h2>
    {#if trend !== undefined}
      <div class="mt-1 block min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-xs text-muted">{trend}</div>
    {/if}
  </div>
{/if}

<style>
  /* Kicker inherits the mono family from the theme bridge; keep the overflow
     ellipsis here because the utility row already truncates the first child. */
  .stat-eyebrow { font-family: var(--font-mono, monospace); }
</style>
