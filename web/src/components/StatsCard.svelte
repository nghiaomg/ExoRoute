<script lang="ts">
  /**
   * Shared dashboard statistic card: icon + headline number, title, one-line
   * trend, and an optional `details-link` action. Tailwind utilities reference
   * the theme bridge in styles/tailwind.css; `!` marks utilities that must
   * out-rank unlayered legacy rules (.dashboard-app button color reset, .icon
   * sizing). The hover layer (gradient wash, icon spin, number pulse) replaces
   * the old `.stat-card` hover rules in the generated shell stylesheet.
   */
  interface Props {
    /** Sprite id for the icon, with or without the leading `#`. */
    icon?: string;
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
    'group/stat relative flex min-w-0 flex-col justify-center overflow-hidden rounded-[17px] bg-surface p-6 ' +
    "after:pointer-events-none after:absolute after:inset-0 after:rounded-[inherit] after:bg-[linear-gradient(120deg,transparent_30%,var(--db-accent-ring)_100%)] after:opacity-0 after:transition-opacity after:duration-200 after:content-[''] group-hover/stat:after:opacity-100 " +
    'max-nav:min-h-[116px] max-nav:rounded-[13px] max-nav:p-4';
  const topClass =
    'flex min-w-0 items-center gap-2.5 @max-[920px]:gap-[7px] max-nav:gap-2';
  const iconClass =
    'grid size-[clamp(29px,3vw,41px)] shrink-0 place-items-center rounded-full bg-db-tint-strong text-muted transition-[background-color,color,transform] duration-200 ' +
    'group-hover/stat:bg-accent group-hover/stat:text-accent-contrast group-hover/stat:scale-[1.08] group-hover/stat:-rotate-6 ' +
    '@max-[920px]:size-7 max-nav:size-[34px]';
  const numberClass =
    'overflow-hidden text-ellipsis text-ink text-kpi! font-bold tabular-nums tracking-[-0.035em] leading-none origin-right ' +
    'transition-[color,transform] duration-200 group-hover/stat:text-accent! group-hover/stat:scale-[1.09]';
  const titleClass =
    'm-0 mt-2 mb-1 overflow-hidden text-ellipsis text-ink text-label font-semibold tracking-[-0.01em] whitespace-nowrap @max-[920px]:mt-2 @max-[920px]:text-label max-nav:whitespace-normal max-nav:overflow-visible';
  const trendClass =
    'block min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-muted text-meta leading-[1.45]';
  const actionClass = `inline-flex w-fit items-center gap-1 mt-1.5 border-0 bg-transparent p-0 text-ink text-meta font-medium text-left cursor-pointer ${focusRing}`;
</script>

{#if skeleton}
  <div class={cardBase} style="min-height:{skeletonHeight}px"></div>
{:else}
  <div class={cardBase}>
    <div class={topClass}>
      <span class={iconClass}><svg class="icon size-[47%]!"><use href={iconHref} /></svg></span>
      <span class={numberClass}>{value}</span>
    </div>
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
  </div>
{/if}
