<script lang="ts">
  import Dialog from '../../components/ui/Dialog.svelte';
  import Button from '../../components/ui/Button.svelte';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Translate } from '../../lib/format';
  interface Props { tr: Translate; report: string; open: boolean; onClose: () => void }
  let { tr, report, open, onClose }: Props = $props();
  async function copy(): Promise<void> {
    try { await navigator.clipboard.writeText(report); showDashboardToast(tr('Diagnostic bundle copied')); }
    catch { showDashboardToast(tr('Copy failed')); }
  }
</script>
<Dialog {open} title={tr('Diagnostic bundle')} closeLabel={tr('Close dialog')} onClose={onClose}>
  <div class="diag-preview">
    <p class="diag-note">🔒 {tr('No secrets are included in this report.')}</p>
    <pre><code>{report}</code></pre>
    <div class="diag-actions"><Button class="page-action" type="button" onclick={onClose}>{tr('Close')}</Button><Button class="page-action primary" type="button" onclick={copy}>{tr('Copy diagnostic bundle')}</Button></div>
  </div>
</Dialog>
<style>
  .diag-preview { display:grid; gap:12px; }
  .diag-note { margin:0; color:var(--chart-good); font-size:12px; }
  .diag-preview pre { margin:0; padding:14px; overflow:auto; max-height:320px; border:1px solid var(--line); border-radius:10px; background:var(--surface-sunken); color:var(--ink); font:11.5px/1.5 var(--font-mono); white-space:pre-wrap; overflow-wrap:anywhere; }
  .diag-actions { display:flex; flex-wrap:wrap; gap:8px; justify-content:flex-end; }
</style>
