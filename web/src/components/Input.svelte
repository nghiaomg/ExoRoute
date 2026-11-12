<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';

  type InputEventHandler = NonNullable<HTMLInputAttributes['oninput']>;
  type ChangeEventHandler = NonNullable<HTMLInputAttributes['onchange']>;
  /* Kept for source compatibility: former Ark-backed variants (combobox,
     password, field) now render through the same native branch. */
  type InputVariant = 'native' | 'combobox' | 'password' | 'field';

  interface Props extends Omit<HTMLInputAttributes, 'type' | 'value' | 'checked' | 'class' | 'oninput' | 'onchange'> {
    type?: HTMLInputAttributes['type'];
    value?: string | number;
    checked?: boolean;
    group?: string | number;
    class?: string;
    element?: HTMLInputElement | null;
    variant?: InputVariant;
    /** Skip the sketch control skin — for inputs nested in a styled container. */
    bare?: boolean;
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
    bare = false,
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

  /* Sketch Input.html control language (variant 01): 12px radius, generous
     padding, focus = full accent border + 15% accent ring. checkbox/radio/
     file/range/hidden stay bare like the sketch's non-text controls. */
  const controlClass =
    'w-full rounded-xl border border-line bg-surface px-3.5 py-3 text-sm text-ink outline-none transition-[border-color,box-shadow] duration-150 placeholder:text-muted focus:border-accent! focus:ring-3! focus:ring-accent/15!';
</script>

{#if type === 'checkbox'}
  <input bind:this={element} type="checkbox" bind:checked value={value ?? undefined} class={className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={oninput} onchange={handleChange} />
{:else if type === 'radio'}
  <input bind:this={element} type="radio" bind:group value={value ?? undefined} class={className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={oninput} onchange={forwardChange} />
{:else if type === 'file'}
  <input bind:this={element} type="file" class={className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={oninput} onchange={forwardChange} />
{:else if type === 'range' || type === 'hidden'}
  <input bind:this={element} {type} value={String(value ?? '')} class={className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={handleInput} onchange={forwardChange} />
{:else}
  <input bind:this={element} {type} value={String(value ?? '')} class={(bare ? '' : controlClass) + ' ' + className} {...attributes} aria-label={ariaLabel ?? attributes['aria-label']} oninput={handleInput} onchange={forwardChange} />
{/if}
