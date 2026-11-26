<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';

  interface Props {
    label?: string;
    value?: string | number;
    type?: HTMLInputAttributes['type'];
    placeholder?: string;
    required?: boolean;
    disabled?: boolean;
    helperText?: string;
    errorText?: string;
    name?: string;
    autocomplete?: HTMLInputAttributes['autocomplete'];
    minlength?: number;
    maxlength?: number;
    min?: number | string;
    max?: number | string;
    step?: number | string;
    class?: string;
    ariaLabel?: string;
    /** Change callback for callers that own the value outside the component. */
    onValueChange?: (value: string) => void;
  }

  let {
    label = '',
    value = $bindable(''),
    type = 'text',
    placeholder = '',
    required = false,
    disabled = false,
    helperText = '',
    errorText = '',
    name,
    autocomplete,
    minlength,
    maxlength,
    min,
    max,
    step,
    class: className = '',
    ariaLabel,
    onValueChange,
  }: Props = $props();

  /* Same control language as the native Input.svelte branch: 12px radius,
     generous padding, focus = full accent border + 15% accent ring. */
  const controlClass =
    'w-full rounded-xl border border-line bg-surface px-3.5 py-3 text-sm text-ink outline-none transition-[border-color,box-shadow] duration-150 placeholder:text-muted focus:border-accent! focus:ring-3! focus:ring-accent/15!';
</script>

<label class="field {className}">
  {#if label}
    {label}
  {/if}
  <input
    class={controlClass}
    bind:value
    {type}
    {placeholder}
    {required}
    {disabled}
    {name}
    {autocomplete}
    {minlength}
    {maxlength}
    {min}
    {max}
    {step}
    aria-label={ariaLabel || label || undefined}
    aria-invalid={errorText ? 'true' : undefined}
    oninput={(event) => { value = (event.currentTarget as HTMLInputElement).value; onValueChange?.(value); }}
  />
  {#if helperText}
    <small class="text-xs leading-normal text-muted">{helperText}</small>
  {/if}
  {#if errorText}
    <small role="alert" class="text-xs leading-normal text-db-danger">{errorText}</small>
  {/if}
</label>
