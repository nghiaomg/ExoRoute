<script lang="ts">
  import { CircleHelp } from '@lucide/svelte';
  import { type Translate } from '../../lib/format';
  import type { ConfigHelp } from './operational-settings.config';

  export let tr: Translate;
  export let id: string;
  export let title: string;
  export let value = 0;
  export let min: number;
  export let max: number;
  export let unit = '';
  export let hint = '';
  export let help: ConfigHelp | undefined = undefined;
  export let onHelp: (help: ConfigHelp) => void = () => {};
  export let disabled = false;
</script>

<label class="resource-limit-field" for={id}>
  <div class="field-title-row">
    <span>{title}</span>
    {#if help}
      <button
        type="button"
        class="config-help-trigger"
        title={tr('Config guidance and error codes')}
        aria-label={tr('Config guidance and error codes')}
        onclick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onHelp(help);
        }}
      >
        <CircleHelp size={13.5} />
      </button>
    {/if}
  </div>
  <span class="resource-limit-input">
    <input {id} type="number" {min} {max} step="1" bind:value {disabled} />
    {#if unit}<small>{unit}</small>{/if}
  </span>
  {#if hint}<small>{hint}</small>{/if}
</label>
