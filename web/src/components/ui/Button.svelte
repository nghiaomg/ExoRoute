<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  export type ButtonVariant = 'primary' | 'secondary' | 'action' | 'icon' | 'text' | 'custom';
  export type ButtonSize = 'default' | 'compact' | 'small';

  interface Props extends Omit<HTMLButtonAttributes, 'class'> {
    variant?: ButtonVariant;
    size?: ButtonSize;
    class?: string;
    stateClasses?: Record<string, boolean | undefined>;
    children?: Snippet;
  }

  let { variant = 'custom', size = 'default', class: className = '', stateClasses = {}, children, ...attributes }: Props = $props();
</script>

<button
  class={'shared-button button-control button-' + variant + ' button-size-' + size + (variant === 'primary' ? ' primary-button' : variant === 'secondary' ? ' secondary-button' : '') + (className ? ' ' + className : '')}
  class:active={stateClasses.active}
  class:is-active={stateClasses['is-active']}
  class:selected={stateClasses.selected}
  class:open={stateClasses.open}
  class:paused={stateClasses.paused}
  class:is-off={stateClasses['is-off']}
  class:copied={stateClasses.copied}
  class:danger-confirm={stateClasses['danger-confirm']}
  class:system-choice={stateClasses['system-choice']}
  class:theme-preview-catppuccin={stateClasses['theme-preview-catppuccin']}
  class:theme-preview-cyber-neon={stateClasses['theme-preview-cyber-neon']}
  class:theme-preview-linear={stateClasses['theme-preview-linear']}
  class:theme-preview-tokyo-night={stateClasses['theme-preview-tokyo-night']}
  class:theme-preview-minimal-light={stateClasses['theme-preview-minimal-light']}
  {...attributes}
>{@render children?.()}</button>
