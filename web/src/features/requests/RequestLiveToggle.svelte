<script lang="ts">
  import { Pause, Play } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';

  export let tr: Translate;
  export let live: boolean;
  export let lastUpdatedMs: number;
  export let nowMs: number;
  export let onToggle: () => void;

  $: idleSeconds = lastUpdatedMs <= 0 ? null : Math.max(0, Math.round((nowMs - lastUpdatedMs) / 1000));
</script>

<button
  type="button"
  class="request-live-toggle"
  class:paused={!live}
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
</button>

<style>
  /* An ink-framed pill with a hard offset shadow, matching the theme's other
     small framed controls. */
  .request-live-toggle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 14px;
    border: 2px solid var(--ink);
    border-radius: 999px;
    background: #eff9f4;
    box-shadow: 2px 2px 0 var(--ink);
    color: #278e6d;
    font-size: var(--text-2xs);
    font-weight: 700;
    cursor: pointer;
    transition: transform 0.14s ease, box-shadow 0.14s ease, border-color 0.14s ease, background-color 0.14s ease;
  }

  @media (hover: hover) {
    .request-live-toggle:hover {
      transform: translate(-1px, -1px);
      box-shadow: 3px 3px 0 var(--ink);
    }
  }

  .request-live-toggle.paused {
    background: #fff7ed;
    color: #c2410c;
  }

  .request-live-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #36c59b;
    box-shadow: 0 0 0 0 rgba(54, 197, 155, 0.6);
    animation: request-live-pulse 1.8s ease-out infinite;
  }

  .request-live-toggle.paused .request-live-dot {
    background: #ea580c;
    box-shadow: none;
    animation: none;
  }

  .request-live-label {
    letter-spacing: 0.4px;
    text-transform: uppercase;
  }

  .request-live-age {
    color: #32876d;
    font-weight: 500;
  }

  .request-live-toggle.paused .request-live-age {
    color: #9a3412;
  }

  .request-live-icon {
    display: grid;
    place-items: center;
    color: currentColor;
    opacity: 0.85;
  }

  @keyframes request-live-pulse {
    0% {
      box-shadow: 0 0 0 0 rgba(54, 197, 155, 0.55);
    }
    70% {
      box-shadow: 0 0 0 6px rgba(54, 197, 155, 0);
    }
    100% {
      box-shadow: 0 0 0 0 rgba(54, 197, 155, 0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .request-live-dot {
      animation: none;
    }
    .request-live-toggle {
      transition: none;
    }
  }

  :global(:root[data-theme='dark']) .request-live-toggle {
    border-color: rgba(34, 197, 94, 0.3);
    background: rgba(34, 197, 94, 0.12);
    color: #86efac;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.3);
  }

  :global(:root[data-theme='dark']) .request-live-toggle .request-live-age {
    color: #6ee7b7;
  }

  :global(:root[data-theme='dark']) .request-live-toggle.paused {
    border-color: rgba(249, 115, 22, 0.3);
    background: rgba(249, 115, 22, 0.12);
    color: #fed7aa;
  }

  :global(:root[data-theme='dark']) .request-live-toggle.paused .request-live-age {
    color: #fdba74;
  }
</style>
