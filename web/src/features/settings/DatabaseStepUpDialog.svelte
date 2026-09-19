<script lang="ts">
  import { Check, LoaderCircle } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkPasswordInput from '../../components/ArkPasswordInput.svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';

  export let open: boolean;
  export let operation: '' | 'export' | 'import';
  export let tr: Translate;
  export let onClose: () => void;
  export let onAuthorized: (operation: 'export' | 'import', proof: string) => Promise<void>;

  let password = '';
  let busy = false;
  let errorMessage = '';

  async function verifyPassword(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if ((operation !== 'export' && operation !== 'import') || busy || !password) return;
    const authorizedOperation = operation;
    errorMessage = '';
    busy = true;
    try {
      const result = await api.reauthenticateAdmin(
        password,
        authorizedOperation === 'export' ? 'database_export' : 'database_import',
      );
      password = '';
      onClose();
      await onAuthorized(authorizedOperation, result.step_up_token);
    } catch (error) {
      errorMessage = localizedError(error, 'Could not reach the ExoRoute API. Check that the gateway is running and try again.', tr);
    } finally {
      busy = false;
    }
  }

  function close(): void {
    if (busy) return;
    password = '';
    errorMessage = '';
    onClose();
  }
</script>

<ArkDialog
  {open}
  closeLabel={tr('Close dialog')}
  title={tr('Confirm admin password')}
  kicker={tr(operation === 'export' ? 'Database export' : 'Database import')}
  onClose={close}
>
  <form class="modal-form" onsubmit={verifyPassword}>
    <p class="modal-description">{tr(operation === 'export' ? 'Enter your admin password to authorize one database export.' : 'Enter your admin password to authorize this database import.')}</p>
    <ArkPasswordInput label={tr('Admin password')} bind:value={password} autocomplete="current-password" visibilityToggleLabel={tr('Toggle password visibility')} required disabled={busy} />
    {#if errorMessage}<div class="form-error" role="alert">{errorMessage}</div>{/if}
    <div class="modal-actions">
      <button type="button" class="secondary-button" disabled={busy} onclick={close}>{tr('Cancel')}</button>
      <button type="submit" class="primary-button" disabled={busy || !password}>{#if busy}<LoaderCircle size={14} class="spin" />{tr('Verifying password…')}{:else}<Check size={14} />{tr('Continue')}{/if}</button>
    </div>
  </form>
</ArkDialog>
