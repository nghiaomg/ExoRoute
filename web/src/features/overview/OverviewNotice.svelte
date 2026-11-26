<script lang="ts">
  import Button from '../../components/ui/Button.svelte';
  import type { Translate } from '../../lib/format';
  interface Props { tr: Translate; message?: string; onRefresh: () => void; onDismiss: () => void }
  let { tr, message = '', onRefresh, onDismiss }: Props = $props();
</script>
<div class="overview-notice" role="status">
  <svg class="icon" aria-hidden="true"><use href="#i-alert" /></svg>
  <div class="overview-notice-copy">
    <strong>{tr('Telemetry partially available')}</strong>
    <span>{message || tr('Live telemetry is warming up or some events were dropped, so totals may be incomplete.')}</span>
  </div>
  <div class="overview-notice-actions">
    <Button class="mini-add" type="button" onclick={onRefresh}>{tr('Retry now')}</Button>
    <Button class="icon-button" type="button" onclick={onDismiss} aria-label={tr('Dismiss')} title={tr('Dismiss')}>×</Button>
  </div>
</div>
