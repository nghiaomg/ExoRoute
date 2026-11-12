<script lang="ts">
  import { localeOptions, type Locale } from '../lib/i18n';
  import type { Translate } from '../lib/format';

  export let locale: Locale;
  export let tr: Translate;
  export let onLocaleChange: (locale: Locale) => void;

  function handleChange(event: Event & { currentTarget: EventTarget & HTMLSelectElement }): void {
    const candidate = event.currentTarget.value;
    const next = localeOptions.some((option) => option.value === candidate)
      ? (candidate as Locale)
      : undefined;
    if (next && next !== locale) {
      onLocaleChange(next);
    }
  }
</script>

<!-- Native select: the .dashboard-app select skin draws the shared chevron.
     h-9!/rounded-lg! match the topbar pill controls; ! out-ranks the unlayered
     legacy resets. -->
<select
  class="h-8! w-auto! max-w-[160px]! rounded-lg! border! border-line! bg-surface! px-2.5! pr-7! text-[13px]! font-medium! text-ink! outline-none! cursor-pointer! focus:border-accent! focus:ring-3! focus:ring-accent/15! [background-size:11px_11px]!"
  aria-label={tr('Language')}
  title={tr('Language')}
  value={locale}
  onchange={handleChange}
>
  {#each localeOptions as option (option.value)}
    <option value={option.value}>{option.label}</option>
  {/each}
</select>
