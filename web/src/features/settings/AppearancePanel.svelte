<script lang="ts">
  import { Check, Palette } from '@lucide/svelte';
  import { THEMES, type ThemeId, type ThemePreference } from '../../lib/theme';
  import { type Translate } from '../../lib/format';

  export let tr: Translate;
  export let theme: ThemeId;
  export let themePreference: ThemePreference;
  export let onThemeChange: (id: ThemeId) => void;
  export let onThemePreferenceChange: (preference: ThemePreference) => void;

  let focusedPreference: ThemePreference = themePreference;
  $: if (themePreference !== focusedPreference && typeof document !== 'undefined' && !document.activeElement?.matches('[role="radio"]')) {
    focusedPreference = themePreference;
  }

  const entries: Array<{ preference: ThemePreference; theme: (typeof THEMES)[number] | null }> = [
    { preference: 'system', theme: null },
    ...THEMES.map((definition) => ({ preference: definition.id as ThemePreference, theme: definition })),
  ];

  function select(preference: ThemePreference): void {
    focusedPreference = preference;
    if (preference === 'system') onThemePreferenceChange('system');
    else onThemeChange(preference);
  }

  function moveFocus(event: KeyboardEvent, currentIndex: number): void {
    let nextIndex: number | null = null;
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') nextIndex = (currentIndex + 1) % entries.length;
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') nextIndex = (currentIndex - 1 + entries.length) % entries.length;
    else if (event.key === 'Home') nextIndex = 0;
    else if (event.key === 'End') nextIndex = entries.length - 1;
    if (nextIndex === null) return;
    event.preventDefault();
    const preference = entries[nextIndex].preference;
    focusedPreference = preference;
    select(preference);
    document.getElementById(`theme-choice-${preference}`)?.focus();
  }

  function isSelected(preference: ThemePreference): boolean {
    return themePreference === preference;
  }
</script>

<section class="settings-panel">
  <div class="panel-heading">
    <span class="panel-icon"><Palette size={17} /></span>
    <div>
      <h2>{tr('Appearance')}</h2>
      <p>{tr('Pick the dashboard color theme. The choice is saved in this browser only.')}</p>
    </div>
  </div>
  <div class="appearance-theme-grid" role="radiogroup" aria-label={tr('Theme')} tabindex="-1" onkeydown={(event) => {
    const target = event.target as HTMLElement;
    const index = entries.findIndex((entry) => `theme-choice-${entry.preference}` === target.id);
    if (index >= 0) moveFocus(event, index);
  }}>
    {#each entries as entry, index (entry.preference)}
      <button
        id="theme-choice-{entry.preference}"
        type="button"
        class="appearance-theme-card"
        class:system-choice={entry.preference === 'system'}
        class:selected={isSelected(entry.preference)}
        class:theme-preview-catppuccin={entry.preference === 'catppuccin'}
        class:theme-preview-cyber-neon={entry.preference === 'cyber-neon'}
        class:theme-preview-linear={entry.preference === 'linear'}
        class:theme-preview-tokyo-night={entry.preference === 'tokyo-night'}
        class:theme-preview-minimal-light={entry.preference === 'minimal-light'}
        role="radio"
        aria-label={entry.preference === 'system' ? tr('System theme') : entry.theme?.name}
        aria-checked={isSelected(entry.preference)}
        tabindex={focusedPreference === entry.preference ? 0 : -1}
        onclick={() => select(entry.preference)}
      >
        {#if entry.theme}
          <span class="appearance-theme-preview theme-preview-{entry.theme.id}" style="background: {entry.theme.swatches[0]};" aria-hidden="true">
            <span class="appearance-theme-preview-surface" style="background: {entry.theme.swatches[1]};"></span>
            <span class="appearance-theme-preview-accent" style="background: {entry.theme.swatches[2]};"></span>
            <span class="appearance-theme-preview-text" style="background: {entry.theme.swatches[3]};"></span>
          </span>
          <span class="appearance-theme-name">
            {entry.theme.name}
            <span class="appearance-theme-style-tag">{tr(entry.theme.styleTag)}</span>
            {#if isSelected(entry.preference)}<span class="appearance-theme-check"><Check size={13} strokeWidth={2.5} /></span>{/if}
          </span>
          <span class="appearance-theme-desc">{tr(entry.theme.descriptionKey)}</span>
        {:else}
          <span class="appearance-theme-preview system-theme-preview" aria-hidden="true">
            <span class="system-theme-light"></span><span class="system-theme-dark"></span>
            <span class="appearance-theme-preview-accent"></span>
          </span>
          <span class="appearance-theme-name">
            {tr('System theme')}
            <span class="appearance-theme-style-tag">{tr(theme === 'dark' ? 'Currently dark' : 'Currently light')}</span>
            {#if isSelected('system')}<span class="appearance-theme-check"><Check size={13} strokeWidth={2.5} /></span>{/if}
          </span>
          <span class="appearance-theme-desc">{tr('Follow your operating system appearance setting.')}</span>
        {/if}
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
    outline-offset: 3px;
  }

  .appearance-theme-card.system-choice {
    border-style: dashed;
  }

  .system-theme-preview {
    position: relative;
    display: block;
    height: 52px;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    overflow: hidden;
    background: linear-gradient(90deg, #f6f7fb 0 50%, #11121b 50% 100%);
  }

  .system-theme-light, .system-theme-dark {
    position: absolute;
    top: 9px;
    width: 38%;
    height: 17px;
    border-radius: 4px;
  }

  .system-theme-light { left: 7%; background: #fff; border: 1px solid #d7d9e1; }
  .system-theme-dark { right: 7%; background: #202332; border: 1px solid #45485a; }
  .system-theme-preview .appearance-theme-preview-accent {
    left: 7%; bottom: 7px; width: 86%; height: 5px;
    background: linear-gradient(90deg, #f97316 0 48%, #89b4fa 52% 100%);
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
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-sm);
    font-weight: var(--font-semibold);
  }

  .appearance-theme-style-tag {
    margin-left: auto;
    padding: 2px 6px;
    border-radius: var(--radius-xs);
    font-size: 10px;
    font-weight: var(--font-semibold);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
    letter-spacing: 0.2px;
    text-transform: uppercase;
  }

  .appearance-theme-card.theme-preview-catppuccin {
    border-radius: 18px;
  }

  .appearance-theme-card.theme-preview-cyber-neon {
    border-radius: 3px;
  }

  .appearance-theme-card.theme-preview-linear,
  .appearance-theme-card.theme-preview-tokyo-night {
    border-radius: 6px;
  }

  .appearance-theme-card.theme-preview-minimal-light {
    border-radius: 8px;
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

  @media (max-width: 480px) {
    .appearance-theme-grid { grid-template-columns: repeat(auto-fill, minmax(min(100%, 190px), 1fr)); gap: 10px; }
    .appearance-theme-card { min-height: 150px; padding: 12px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .appearance-theme-card { transition: none; }
    .appearance-theme-card:hover { transform: none; }
  }

  .appearance-theme-desc {
    color: var(--muted);
    font-size: var(--text-2xs);
    line-height: var(--leading-snug);
  }
</style>
