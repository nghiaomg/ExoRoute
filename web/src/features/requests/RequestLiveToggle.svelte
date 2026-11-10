<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { Pause, Play } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';

  export let tr: Translate;
  export let live: boolean;
  export let lastUpdatedMs: number;
  export let nowMs: number;
  export let onToggle: () => void;

  $: idleSeconds = lastUpdatedMs <= 0 ? null : Math.max(0, Math.round((nowMs - lastUpdatedMs) / 1000));
</script>

<Button
  type="button"
  class="request-live-toggle" stateClasses={{ paused: !live }}

  aria-pressed={live}
  onclick={onToggle}
>
  <span class="request-live-dot" aria-hidden="true"></span>
  <span class="request-live-label">{live ? tr('Live') : tr('Paused')}</span>
  {#if idleSeconds != null}
    <span class="request-live-age">{tr('Updated {seconds}s ago', { seconds: idleSeconds })}</span>
  {/if}
  <span class="request-live-icon" aria-hidden="true">
    {#if live}<Pause size={13} />{:else}<Play size={13} />{/if}
  </span>
</Button>

<style>
  /* An ink-framed pill with a hard offset shadow, matching the theme's other
     small framed controls. */
  :global(.request-live-toggle) {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 14px;
    border: var(--nb-border);
    border-radius: 999px;
    background: color-mix(in srgb, var(--chart-good) 10%, var(--surface));
    box-shadow: var(--nb-shadow-sm);
    color: var(--chart-good);
    font-size: var(--text-2xs);
    font-weight: 700;
    cursor: pointer;
    transition: transform 0.14s ease, box-shadow 0.14s ease, border-color 0.14s ease, background-color 0.14s ease;
  }

  @media (hover: hover) {
    :global(.request-live-toggle:hover) {
      transform: translate(-1px, -1px);
      box-shadow: var(--nb-shadow-md);
    }
  }

  :global(.request-live-toggle.paused) {
    background: color-mix(in srgb, var(--accent) 10%, var(--surface));
    color: var(--accent-deep);
  }

  .request-live-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--mint);
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--mint) 60%, transparent);
    animation: request-live-pulse 1.8s ease-out infinite;
  }

  :global(.request-live-toggle.paused) .request-live-dot {
    background: var(--accent);
    box-shadow: none;
    animation: none;
  }

  .request-live-label {
    letter-spacing: 0.4px;
    text-transform: uppercase;
  }

  .request-live-age {
    color: var(--chart-good);
    font-weight: 500;
  }

  :global(.request-live-toggle.paused) .request-live-age {
    color: var(--accent-deep);
  }

  .request-live-icon {
    display: grid;
    place-items: center;
    color: currentColor;
    opacity: 0.85;
  }

  @keyframes request-live-pulse {
    0% {
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--mint) 55%, transparent);
    }
    70% {
      box-shadow: 0 0 0 6px color-mix(in srgb, var(--mint) 0%, transparent);
    }
    100% {
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--mint) 0%, transparent);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .request-live-dot {
      animation: none;
    }
    :global(.request-live-toggle) {
      transition: none;
    }
  }

</style>
