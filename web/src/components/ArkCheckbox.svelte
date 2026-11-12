<script lang="ts">
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

  let inputEl = $state<HTMLInputElement | null>(null);

  /* The prop is the source of truth (callback-driven, not bindable): reflect
     it onto the native control, including the indeterminate state which has
     no HTML attribute. `!` out-ranks the unlayered legacy `.modal-form input`
     reset, which would otherwise stretch the box to a full-width 37px control. */
  $effect(() => {
    if (!inputEl) return;
    inputEl.checked = checked === true;
    inputEl.indeterminate = checked === 'indeterminate';
  });

  function handleChange(event: Event & { currentTarget: EventTarget & HTMLInputElement }): void {
    onCheckedChange(event.currentTarget.checked);
  }
</script>

<label
  class="inline-flex! items-center! gap-2! select-none {disabled ? 'cursor-not-allowed! opacity-55' : 'cursor-pointer!'} {className}"
>
  <input
    bind:this={inputEl}
    type="checkbox"
    class="h-[17px]! w-[17px]! shrink-0 accent-accent!"
    {disabled}
    aria-label={ariaLabel || label || undefined}
    onchange={handleChange}
  />
  {#if label || description}
    <span class="grid! gap-[3px]!">
      {#if label}
        <span class="text-sm! font-semibold! text-ink!">{label}</span>
      {/if}
      {#if description}
        <small class="text-xs! leading-normal! text-muted!">{description}</small>
      {/if}
    </span>
  {/if}
</label>
