<script lang="ts">
  import { Combobox } from '@ark-ui/svelte/combobox';
  import { Field } from '@ark-ui/svelte/field';
  import { PasswordInput } from '@ark-ui/svelte/password-input';
  import type { HTMLInputAttributes } from 'svelte/elements';

  type InputEventHandler = NonNullable<HTMLInputAttributes['oninput']>;
  type ChangeEventHandler = NonNullable<HTMLInputAttributes['onchange']>;
  type InputVariant = 'native' | 'combobox' | 'password' | 'field';

  interface Props extends Omit<HTMLInputAttributes, 'type' | 'value' | 'checked' | 'class' | 'oninput' | 'onchange'> {
    type?: HTMLInputAttributes['type'];
    value?: string | number;
    checked?: boolean;
    group?: string | number;
    class?: string;
    element?: HTMLInputElement | null;
    variant?: InputVariant;
    oninput?: InputEventHandler;
    onchange?: ChangeEventHandler;
    ariaLabel?: string;
  }

  let {
    type = 'text',
    value = $bindable<string | number | undefined>(),
    checked = $bindable(false),
    group = $bindable<string | number>(),
    class: className = '',
    element = $bindable<HTMLInputElement | null>(null),
    variant = 'native',
    oninput,
    onchange,
    ariaLabel,
    ...attributes
  }: Props = $props();

  function handleInput(event: Event & { currentTarget: EventTarget & HTMLInputElement }): void {
    value = type === 'number' || type === 'range' ? event.currentTarget.valueAsNumber : event.currentTarget.value;
    oninput?.(event);
  }

  function handleChange(event: Event & { currentTarget: EventTarget & HTMLInputElement }): void {
    checked = event.currentTarget.checked;
    onchange?.(event);
  }

  function forwardChange(event: Event & { currentTarget: EventTarget & HTMLInputElement }): void {
    onchange?.(event);
  }
</script>

{#if variant === 'combobox'}
  <Combobox.Input class={'input-control ' + className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} {oninput} {onchange} />
{:else if variant === 'password'}
  <PasswordInput.Input bind:value class={'input-control ' + className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} {oninput} {onchange} />
{:else if variant === 'field'}
  <Field.Input bind:value {type} class={'input-control ' + className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={handleInput} {onchange} />
{:else if type === 'checkbox'}
  <input bind:this={element} type="checkbox" bind:checked value={value ?? undefined} class={'input-control ' + className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={oninput} onchange={handleChange} />
{:else if type === 'radio'}
  <input bind:this={element} type="radio" bind:group value={value ?? undefined} class={'input-control ' + className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={oninput} onchange={forwardChange} />
{:else if type === 'file'}
  <input bind:this={element} type="file" class={'input-control ' + className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={oninput} onchange={forwardChange} />
{:else}
  <input bind:this={element} {type} value={String(value ?? '')} class={'input-control ' + className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={handleInput} onchange={forwardChange} />
{/if}
