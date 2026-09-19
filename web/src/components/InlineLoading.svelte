<script lang="ts">
  import { LoaderCircle } from '@lucide/svelte';
  import type { Translate } from '../lib/format';

  export let label: string;
  export let tr: Translate;
  export let vars: Record<string, string | number> = {};
</script>

<div class="inline-loading" role="status"><LoaderCircle size={18} class="spin" /><span>{tr(label, vars)}</span></div>
