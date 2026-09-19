<script lang="ts">
  import { Activity, Boxes, Cpu, KeyRound, Layers3, Plus, Search } from '@lucide/svelte';

  export let icon: 'search' | 'providers' | 'combos' | 'models' | 'api-keys' | 'requests' = 'requests';
  export let title: string;
  export let description: string;
  export let action: string | undefined = undefined;
  export let onclick: (() => void) | undefined = undefined;
</script>

<section class="empty-state">
  <span class="empty-illustration">
    <span class="empty-orbit"></span>
    {#if icon === 'search'}<Search size={21} />
    {:else if icon === 'providers'}<Boxes size={21} />
    {:else if icon === 'combos'}<Layers3 size={21} />
    {:else if icon === 'models'}<Cpu size={21} />
    {:else if icon === 'api-keys'}<KeyRound size={21} />
    {:else}<Activity size={21} />{/if}
  </span>
  <h2>{title}</h2>
  <p>{description}</p>
  {#if action && onclick}<button class="primary-button" onclick={onclick}><Plus size={16} />{action}</button>{/if}
</section>
