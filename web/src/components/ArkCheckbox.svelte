<script lang="ts">
  import { Checkbox } from '@ark-ui/svelte/checkbox';
  import { Check, Minus } from '@lucide/svelte';

  interface Props {
    checked?: boolean | 'indeterminate';
    disabled?: boolean;
    label?: string;
    description?: string;
    class?: string;
    ariaLabel?: string;
    onCheckedChange?: (checked: boolean | 'indeterminate') => void;
  }

  let {
    checked = false,
    disabled = false,
    label = '',
    description = '',
    class: className = '',
    ariaLabel = '',
    onCheckedChange = () => {},
  }: Props = $props();
</script>

<Checkbox.Root
  {checked}
  {disabled}
  onCheckedChange={(details) => onCheckedChange(details.checked)}
  class="ark-checkbox-root {className}"
>
  <Checkbox.Control class="ark-checkbox-control">
    <Checkbox.Indicator class="ark-checkbox-indicator">
      {#if checked === 'indeterminate'}
        <Minus size={12} strokeWidth={3} />
      {:else}
        <Check size={12} strokeWidth={3} />
      {/if}
    </Checkbox.Indicator>
  </Checkbox.Control>
  {#if label || description}
    <div class="ark-checkbox-text">
      {#if label}
        <Checkbox.Label class="ark-checkbox-label">{label}</Checkbox.Label>
      {/if}
      {#if description}
        <small class="ark-checkbox-description">{description}</small>
      {/if}
    </div>
  {/if}
  <Checkbox.HiddenInput aria-label={ariaLabel || label} />
</Checkbox.Root>
