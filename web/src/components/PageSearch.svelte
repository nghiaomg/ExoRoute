<script lang="ts">
  import { onMount } from 'svelte';
  import { Search } from '@lucide/svelte';
  import type { Translate } from '../lib/format';

  export let value = '';
  export let pageLabel: string;
  export let tr: Translate;
  export let onInput: (value: string) => void = () => {};

  let input: HTMLInputElement;
  let isMac = false;

  function getPlaceholder(label: string): string {
    const lower = label.toLowerCase().replace(/\bapi\b/g, 'API');
    return tr('Search {page}…', { page: lower });
  }

  function getAriaLabel(label: string): string {
    return tr('Search {page}…', { page: label });
  }

  onMount(() => {
    isMac = typeof navigator !== 'undefined' && /(Mac|iPhone|iPod|iPad)/i.test(navigator.platform || navigator.userAgent || '');
    const focusOnShortcut = (event: KeyboardEvent): void => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        input?.focus();
      }
    };
    window.addEventListener('keydown', focusOnShortcut);
    return () => window.removeEventListener('keydown', focusOnShortcut);
  });
</script>

<label class="search-box">
  <Search size={16} />
  <input
    bind:value
    bind:this={input}
    oninput={() => onInput(value)}
    placeholder={getPlaceholder(pageLabel)}
    aria-label={getAriaLabel(pageLabel)}
  />
  <kbd>{isMac ? '⌘ K' : 'Ctrl K'}</kbd>
</label>

<style>
  .search-box {
    height: 36px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    color: #9b9eae;
    border: none !important;
    border-radius: 8px;
    background: #f0f2f8;
    box-shadow: none !important;
    transition: background 0.16s ease, color 0.16s ease;
    cursor: text;
  }
  .search-box:focus-within {
    background: #e8ebf4;
    color: var(--violet, #ea580c);
  }
  .search-box input {
    width: 175px;
    padding: 0;
    color: #45495c;
    border: none !important;
    outline: none !important;
    background: transparent !important;
    font-size: 12px;
    font-family: inherit;
    box-shadow: none !important;
  }
  .search-box input::placeholder {
    color: #a7a9b6;
  }
  .search-box kbd {
    padding: 2px 5px;
    color: #636679;
    border: none !important;
    border-radius: 4px;
    background: #e2e4ef;
    font: 500 12px var(--font-sans);
    line-height: 1.2;
    white-space: nowrap;
    box-shadow: none !important;
  }

  :global(:root[data-theme='dark']) .search-box {
    color: #a5a8ba;
    background: #181927 !important;
    border: none !important;
    box-shadow: none !important;
  }
  :global(:root[data-theme='dark']) .search-box:focus-within {
    background: #1f2032 !important;
    color: #fdba74;
  }
  :global(:root[data-theme='dark']) .search-box input {
    color: #f0edff !important;
    border: none !important;
    background: transparent !important;
  }
  :global(:root[data-theme='dark']) .search-box input::placeholder {
    color: #85899d;
  }
  :global(:root[data-theme='dark']) .search-box kbd {
    background: #27283c !important;
    color: #9da0b5 !important;
    border: none !important;
  }

  @media (max-width: 620px) {
    .search-box {
      width: 100%;
    }
    .search-box input {
      width: 100%;
    }
  }
</style>
