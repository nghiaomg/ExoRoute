<script lang="ts">
  import { Clipboard } from '@ark-ui/svelte/clipboard';
  import { Check, Copy } from '@lucide/svelte';

  interface Props {
    value: string;
    copyLabel?: string;
    copiedLabel?: string;
    class?: string;
  }

  let {
    value,
    copyLabel = 'Copy key',
    copiedLabel = 'Copied!',
    class: className = '',
  }: Props = $props();
</script>

<Clipboard.Root {value} class="ark-clipboard-root {className}">
  <Clipboard.Control class="ark-clipboard-control">
    <Clipboard.Trigger class="secondary-button ark-clipboard-trigger">
      <Clipboard.Indicator>
        {#snippet copied()}
          <Check size={14} class="ark-clipboard-check" />
          <span>{copiedLabel}</span>
        {/snippet}
        <Copy size={14} />
        <span>{copyLabel}</span>
      </Clipboard.Indicator>
    </Clipboard.Trigger>
  </Clipboard.Control>
</Clipboard.Root>
