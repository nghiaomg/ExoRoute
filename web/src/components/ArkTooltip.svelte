<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Tooltip } from '@ark-ui/svelte/tooltip';

  interface Props {
    content: string;
    class?: string;
    openDelay?: number;
    closeDelay?: number;
    children?: Snippet;
  }

  let {
    content,
    class: className = '',
    openDelay = 200,
    closeDelay = 100,
    children,
  }: Props = $props();
</script>

<Tooltip.Root {openDelay} {closeDelay}>
  <Tooltip.Trigger class="ark-tooltip-trigger {className}">
    {@render children?.()}
  </Tooltip.Trigger>
  <Tooltip.Positioner class="ark-tooltip-positioner">
    <Tooltip.Content class="ark-tooltip-content">
      {content}
    </Tooltip.Content>
  </Tooltip.Positioner>
</Tooltip.Root>
