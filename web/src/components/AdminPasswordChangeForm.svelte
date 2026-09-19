<script lang="ts">
  import { Check, LoaderCircle } from '@lucide/svelte';
  import ArkPasswordInput from './ArkPasswordInput.svelte';
  import { api, type AdminAccessResult } from '../lib/api';
  import { type Translate } from '../lib/format';
import { localizedError } from '../lib/errors';

  export let tr: Translate;
  export let forced = false;
  export let initialCurrentPassword = '';
  export let onComplete: (result: AdminAccessResult) => void;
  export let onCancel: () => void = () => {};

  let currentPassword = initialCurrentPassword;
  let newPassword = '';
  let confirmPassword = '';
  let saving = false;
  let errorMessage = '';

  $: if (forced) currentPassword = initialCurrentPassword;

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    errorMessage = '';
    if (newPassword !== confirmPassword) {
      errorMessage = tr('The new passwords do not match.');
      return;
    }
    saving = true;
    try {
      const result = await api.changeAdminPassword(currentPassword, newPassword);
      currentPassword = '';
      newPassword = '';
      confirmPassword = '';
      onComplete(result);
    } catch (error) {
      errorMessage = localizedError(error, 'Could not change the admin password.', tr);
    } finally {
      saving = false;
    }
  }
</script>

<form class:login-form={forced} class="modal-form" onsubmit={submit}>
  <p class="modal-description">{tr(forced ? 'Your current admin password is a default or weak value. Choose a new password now to unlock ExoRoute.' : 'The new password is stored as a salted hash in the database and included in database backups. Use at least 12 characters and choose a unique phrase.')}</p>
  {#if !forced}<ArkPasswordInput label={tr('Current password')} bind:value={currentPassword} autocomplete="current-password" visibilityToggleLabel={tr('Toggle password visibility')} required />{/if}
  <ArkPasswordInput label={tr('New password')} bind:value={newPassword} autocomplete="new-password" minlength={12} maxlength={128} visibilityToggleLabel={tr('Toggle password visibility')} required />
  <ArkPasswordInput label={tr('Confirm new password')} bind:value={confirmPassword} autocomplete="new-password" minlength={12} maxlength={128} visibilityToggleLabel={tr('Toggle password visibility')} required />
  {#if errorMessage}<div class="form-error" role="alert">{errorMessage}</div>{/if}
  <div class="modal-actions">
    {#if !forced}<button type="button" class="secondary-button" onclick={onCancel}>{tr('Cancel')}</button>{/if}
    <button class="primary-button" disabled={saving}>{#if saving}<LoaderCircle size={15} class="spin" />{:else}<Check size={15} />{/if}{tr(forced ? 'Set new password' : 'Save password')}</button>
  </div>
</form>
