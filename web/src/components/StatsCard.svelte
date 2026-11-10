<script lang="ts">
  import Button from './Button.svelte';

  /**
   * Shared dashboard statistic card: icon + headline number, title, one-line
   * trend, and an optional `details-link` action. Renders a <div> (the card is
   * part of a section, not standalone content) with the same `.stat-card`
   * structure every dashboard page used to repeat inline.
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
</script>

{#if skeleton}
  <div class="stat-card surface" style="min-height:{skeletonHeight}px"></div>
{:else}
  <div class="stat-card surface">
    <div class="stat-top">
      <span class="stat-icon"><svg class="icon"><use href={iconHref} /></svg></span>
      <span class="stat-number">{value}</span>
    </div>
    <h2>{title}</h2>
    {#if trend !== undefined}
      <div class="stat-trend">{trend}</div>
    {/if}
    {#if actionLabel !== undefined}
      <Button class="details-link" type="button" onclick={onAction}>
        {actionLabel}
        <svg class="icon"><use href="#i-arrow-right" /></svg>
      </Button>
    {/if}
  </div>
{/if}
