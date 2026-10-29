<script lang="ts">
  import { Check, Palette } from '@lucide/svelte';
  import { THEMES, type ThemeId } from '../../lib/theme';
  import { type Translate } from '../../lib/format';

  export let tr: Translate;
  export let theme: ThemeId;
  export let onThemeChange: (id: ThemeId) => void;
</script>

<section class="settings-panel">
  <div class="panel-heading">
    <span class="panel-icon"><Palette size={17} /></span>
    <div>
      <h2>{tr('Appearance')}</h2>
      <p>{tr('Pick the dashboard color theme. The choice is saved in this browser only.')}</p>
    </div>
  </div>
  <div class="appearance-theme-grid" role="radiogroup" aria-label={tr('Theme')}>
    {#each THEMES as def (def.id)}
      <button
        type="button"
        class="appearance-theme-card"
        class:selected={theme === def.id}
        role="radio"
        aria-checked={theme === def.id}
        onclick={() => onThemeChange(def.id)}
      >
        <span class="appearance-theme-preview" style="background: {def.swatches[0]};" aria-hidden="true">
          <span class="appearance-theme-preview-surface" style="background: {def.swatches[1]};"></span>
          <span class="appearance-theme-preview-accent" style="background: {def.swatches[2]};"></span>
          <span class="appearance-theme-preview-text" style="background: {def.swatches[3]};"></span>
        </span>
        <span class="appearance-theme-name">
          {def.name}
          {#if theme === def.id}<span class="appearance-theme-check"><Check size={13} strokeWidth={2.5} /></span>{/if}
        </span>
        <span class="appearance-theme-desc">{tr(def.descriptionKey)}</span>
      </button>
    {/each}
  </div>
</section>

<style>
  .appearance-theme-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(215px, 1fr));
    gap: 12px;
  }

  .appearance-theme-card {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 7px;
    padding: 12px;
    border: 2px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--paper);
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: border-color 0.14s ease, box-shadow 0.14s ease, transform 0.14s ease;
  }

  .appearance-theme-card:hover {
    border-color: var(--accent);
    transform: translateY(-1px);
  }

  .appearance-theme-card.selected {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }

  .appearance-theme-card:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .appearance-theme-preview {
    position: relative;
    display: block;
    height: 52px;
    border-radius: var(--radius-md);
    border: 1px solid rgba(0, 0, 0, 0.14);
    overflow: hidden;
  }

  .appearance-theme-preview-surface {
    position: absolute;
    inset: 8px 12px auto;
    height: 18px;
    display: block;
    border-radius: 5px;
    border: 1px solid rgba(0, 0, 0, 0.12);
  }

  .appearance-theme-preview-accent {
    position: absolute;
    left: 12px;
    bottom: 8px;
    width: 26px;
    height: 8px;
    display: block;
    border-radius: var(--radius-full);
  }

  .appearance-theme-preview-text {
    position: absolute;
    left: 46px;
    bottom: 10px;
    width: 44px;
    height: 4px;
    display: block;
    border-radius: var(--radius-full);
    opacity: 0.85;
  }

  .appearance-theme-name {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-sm);
    font-weight: var(--font-semibold);
  }

  .appearance-theme-check {
    display: inline-grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: var(--radius-full);
    color: #fff;
    background: var(--accent);
  }

  .appearance-theme-desc {
    color: var(--muted);
    font-size: var(--text-2xs);
    line-height: var(--leading-snug);
  }
</style>
