<script lang="ts">
  import type { Provider } from '../../../src/lib/types';

  let { provider }: { provider: Provider } = $props();
</script>

<section data-testid="provider-detail" data-provider-id={provider.id}>
  <h1>{provider.name}</h1>
</section>
