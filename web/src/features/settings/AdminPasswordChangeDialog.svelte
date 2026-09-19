<script lang="ts">
  import ArkDialog from '../../components/ArkDialog.svelte';
  import AdminPasswordChangeForm from '../../components/AdminPasswordChangeForm.svelte';
  import type { AdminAccessResult } from '../../lib/api';
  import type { Translate } from '../../lib/format';

  export let open = false;
  export let tr: Translate;
  export let onClose: () => void;
  export let onComplete: (result: AdminAccessResult) => void;
</script>

<ArkDialog {open} closeLabel={tr('Close dialog')} title={tr('Change admin password')} kicker={tr('EXOROUTE CONTROL PLANE')} onClose={onClose}>
  <AdminPasswordChangeForm {tr} onComplete={onComplete} onCancel={onClose} />
</ArkDialog>
