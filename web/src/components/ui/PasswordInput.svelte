<script lang="ts">
  import { Eye, EyeOff } from '@lucide/svelte';
  import type { HTMLInputAttributes } from 'svelte/elements';

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

  let visible = $state(false);
</script>

<label class="field {className}">
  {#if label}
    {label}
  {/if}
  <span class="relative block">
    <input
      type={visible ? 'text' : 'password'}
      class="w-full rounded-xl border border-line bg-surface px-3.5 py-3 pr-10! text-sm text-ink outline-none transition-[border-color,box-shadow] duration-150 placeholder:text-muted focus:border-accent! focus:ring-3! focus:ring-accent/15!"
      bind:value
      {placeholder}
      {name}
      {autocomplete}
      {minlength}
      {maxlength}
      {required}
      {disabled}
      aria-label={ariaLabel || label || undefined}
    />
    <button
      type="button"
      class="absolute right-2 top-1/2 -translate-y-1/2 grid place-items-center size-7 rounded-md text-muted hover:text-ink {disabled ? 'cursor-not-allowed' : 'cursor-pointer'}"
      aria-label={visibilityToggleLabel}
      aria-pressed={visible}
      {disabled}
      onclick={() => (visible = !visible)}
    >
      {#if visible}
        <Eye size={15} />
      {:else}
        <EyeOff size={15} />
      {/if}
    </button>
  </span>
</label>
