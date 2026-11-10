<script lang="ts">
  import { PasswordInput } from '@ark-ui/svelte/password-input';
  import { Eye, EyeOff } from '@lucide/svelte';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import Input from './Input.svelte';

  interface Props {
    label?: string;
    value?: string;
    placeholder?: string;
    required?: boolean;
    disabled?: boolean;
    name?: string;
    autocomplete?: HTMLInputAttributes['autocomplete'];
    minlength?: number;
    maxlength?: number;
    visibilityToggleLabel?: string;
    class?: string;
    ariaLabel?: string;
  }

  let {
    label = '',
    value = $bindable(''),
    placeholder = '••••••••••••',
    required = false,
    disabled = false,
    name,
    autocomplete = 'current-password',
    minlength,
    maxlength,
    visibilityToggleLabel = 'Toggle password visibility',
    class: className = '',
    ariaLabel,
  }: Props = $props();
</script>

<PasswordInput.Root {disabled} {required} class={`ark-password-root ${className}`}>
  {#if label}
    <PasswordInput.Label class="ark-password-label">{label}</PasswordInput.Label>
  {/if}
  <PasswordInput.Control class="ark-password-control">
    <Input
      bind:value
      class="ark-password-input"
      variant="password"
      type="password"
      {placeholder}
      {name}
      {autocomplete}
      {minlength}
      {maxlength}
      {required}
      {disabled}
      aria-label={ariaLabel || label}
    />
    <PasswordInput.VisibilityTrigger class="ark-password-trigger" type="button" aria-label={visibilityToggleLabel}>
      <PasswordInput.Indicator class="ark-password-indicator">
        {#snippet fallback()}
          <EyeOff size={15} />
        {/snippet}
        <Eye size={15} />
      </PasswordInput.Indicator>
    </PasswordInput.VisibilityTrigger>
  </PasswordInput.Control>
</PasswordInput.Root>
