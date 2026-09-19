<script lang="ts">
  import { Radio, RefreshCw } from '@lucide/svelte';
  import type { Translate } from '../lib/format';

  export let message: string;
  export let tr: Translate;
  export let onRetry: () => void;
</script>

<section class="state-card error-state">
  <div class="state-icon error-icon"><Radio size={21} /></div>
  <div class="state-copy">
    <h2>{tr('We couldn’t reach your gateway')}</h2>
    <p>{message}</p>
    <div class="state-hint"><code>/api/v1/admin</code><span>{tr('Check the server, then try again.')}</span></div>
  </div>
  <button class="secondary-button" onclick={onRetry}><RefreshCw size={15} />{tr('Retry')}</button>
</section>
