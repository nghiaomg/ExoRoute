<script lang="ts">
  import { Field } from '@ark-ui/svelte/field';
  import type { HTMLInputAttributes } from 'svelte/elements';

  interface Props {
    label?: string;
    value?: string;
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

  function handleInput(event: Event & { currentTarget: EventTarget & HTMLInputElement }): void {
    value = event.currentTarget.value;
    onValueChange?.(value);
  }
</script>

<Field.Root {disabled} {required} invalid={Boolean(errorText)} class={`ark-field-root ${className}`}>
  {#if label}
    <Field.Label class="ark-field-label">{label}</Field.Label>
  {/if}
  <Field.Input
    bind:value
    {type}
    class="ark-field-input"
    {placeholder}
    {name}
    {autocomplete}
    {minlength}
    {maxlength}
    {min}
    {max}
    {step}
    aria-label={ariaLabel || label}
    oninput={handleInput}
  />
  {#if helperText}
    <Field.HelperText class="ark-field-helper">{helperText}</Field.HelperText>
  {/if}
  {#if errorText}
    <Field.ErrorText class="ark-field-error">{errorText}</Field.ErrorText>
  {/if}
</Field.Root>
