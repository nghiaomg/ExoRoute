<script lang="ts">
  import { Check, Database, Download, LoaderCircle, Upload } from '@lucide/svelte';
  import ArkCheckbox from '../../components/ArkCheckbox.svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import DatabaseStepUpDialog from './DatabaseStepUpDialog.svelte';
  import { api, ApiError } from '../../lib/api';
  import { formatFileSize, type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';

  export let tr: Translate;
  export let onAuthenticationReset: () => void;
  export let onImported: () => void;

  let fileInput: HTMLInputElement;
  let databaseFile: File | null = null;
  let busy: '' | 'export' | 'import' = '';
  let includeRequestLogs = true;
  let errorMessage = '';
  let successMessage = '';
  let importDialogOpen = false;
  let confirmUnlimitedImport = false;
  let stepUpOperation: '' | 'export' | 'import' = '';

  function handleFileSelection(event: Event): void {
    const input = event.currentTarget as HTMLInputElement;
    const selectedFile = input.files?.[0] ?? null;
    errorMessage = '';
    successMessage = '';
    confirmUnlimitedImport = false;
    if (selectedFile && selectedFile.size > 512 * 1024 * 1024) {
      databaseFile = null;
      errorMessage = tr('The database file exceeds the 512 MB upload limit.');
      input.value = '';
      return;
    }
    databaseFile = selectedFile;
  }

  function requestExport(): void {
    errorMessage = '';
    successMessage = '';
    stepUpOperation = 'export';
  }

  function requestImport(): void {
    if (!databaseFile || busy) return;
    errorMessage = '';
    successMessage = '';
    importDialogOpen = true;
  }

  async function importDatabase(): Promise<void> {
    if (!databaseFile || busy) return;
    importDialogOpen = false;
    errorMessage = '';
    successMessage = '';
    stepUpOperation = 'import';
  }

  async function runAuthorizedOperation(operation: 'export' | 'import', proof: string): Promise<void> {
    busy = operation;
    try {
      stepUpOperation = '';
      if (operation === 'export') {
        const { blob, filename } = await api.exportDatabase(includeRequestLogs, proof);
        const url = URL.createObjectURL(blob);
        const link = document.createElement('a');
        link.href = url;
        link.download = filename;
        document.body.append(link);
        link.click();
        link.remove();
        window.setTimeout(() => URL.revokeObjectURL(url), 1000);
        successMessage = tr('Database backup downloaded.');
      } else {
        if (!databaseFile) throw new ApiError(tr('Choose a database backup before importing.'), 400);
        const result = await api.importDatabase(databaseFile, proof, confirmUnlimitedImport);
        if (!result.ok) throw new ApiError(tr('Could not import the database.'), 500);
        databaseFile = null;
        confirmUnlimitedImport = false;
        if (fileInput) fileInput.value = '';
        successMessage = tr('Database imported successfully.');
        if (result.authentication_reset) onAuthenticationReset();
        else onImported();
      }
    } catch (error) {
      const message = localizedError(error, operation === 'export' ? 'Could not export the database.' : 'Could not import the database.', tr);
      errorMessage = message;
    } finally {
      busy = '';
    }
  }

  function cancelStepUp(): void {
    stepUpOperation = '';
  }
</script>

<section class="settings-panel settings-wide database-panel">
  <div class="panel-heading"><span class="panel-icon violet-panel"><Database size={17} /></span><div><h2>{tr('Database backup')}</h2><p>{tr('Database backups are ExoRoute LMDB archives that include configuration, API keys, optionally request logs, and encrypted provider secrets.')}</p></div></div>
  <div class="database-content">
    <div class="database-log-option"><ArkCheckbox checked={includeRequestLogs} disabled={busy !== ''} label={tr('Include request logs in export')} description={tr('When unchecked, request logs and traffic statistics are omitted, and lifetime API key counts reset in the exported backup.')} onCheckedChange={(checked) => includeRequestLogs = checked === true} /></div>
    <div class="database-actions">
      <button class="secondary-button" disabled={busy !== ''} onclick={requestExport}>{#if busy === 'export'}<LoaderCircle size={14} class="spin" />{tr('Exporting database…')}{:else}<Download size={14} />{tr('Export database')}{/if}</button>
      <label class="secondary-button database-file-label" class:has-file={databaseFile !== null}><Upload size={14} />{tr('Choose database file')}<input class="database-file-input" type="file" accept=".exoroute,application/vnd.exoroute.lmdb-backup" aria-label={tr('Choose database file')} disabled={busy !== ''} onchange={handleFileSelection} bind:this={fileInput} /></label>
      <button class="primary-button" disabled={!databaseFile || busy !== ''} onclick={requestImport}>{#if busy === 'import'}<LoaderCircle size={14} class="spin" />{tr('Importing database…')}{:else}<Upload size={14} />{tr('Import database')}{/if}</button>
    </div>
    <div class="database-feedback" aria-live="polite">
      {#if busy}<span class="database-progress"><LoaderCircle size={13} class="spin" />{tr(busy === 'export' ? 'Exporting database…' : 'Importing database…')}</span>
      {:else if errorMessage}<span class="database-error" role="alert">{errorMessage}</span>
      {:else if successMessage}<span class="database-success" role="status"><Check size={13} />{successMessage}</span>
      {:else if databaseFile}<span>{tr('Selected file: {name} ({size})', { name: databaseFile.name, size: formatFileSize(databaseFile.size) })}</span>
      {:else}<span>{tr('No database file selected')}</span>{/if}
    </div>
  </div>
</section>

<ArkDialog
  open={importDialogOpen}
  closeLabel={tr('Close dialog')}
  title={tr('Confirm database import')}
  kicker={tr('Database backup')}
  onClose={() => importDialogOpen = false}
>
  <div class="modal-form">
    <div class="resource-limits-notice" role="alert">{tr('Importing this backup replaces all current gateway data. This cannot be undone.')}</div>
    <div class="resource-limits-notice" role="alert">{tr('Imported settings may enable unlimited total gateway or per-provider concurrency, which can exhaust RAM, CPU, or file descriptors.')}</div>
    <ArkCheckbox
      checked={confirmUnlimitedImport}
      label={tr('I accept the unlimited-concurrency resource risk if this backup enables it.')}
      disabled={busy !== ''}
      onCheckedChange={(checked) => confirmUnlimitedImport = checked === true}
    />
    <div class="modal-actions">
      <button type="button" class="secondary-button" onclick={() => importDialogOpen = false}>{tr('Cancel')}</button>
      <button type="button" class="primary-button" disabled={busy !== ''} onclick={importDatabase}>{#if busy === 'import'}<LoaderCircle size={14} class="spin" />{tr('Importing database…')}{:else}<Upload size={14} />{tr('Import database')}{/if}</button>
    </div>
  </div>
</ArkDialog>

<DatabaseStepUpDialog
  open={stepUpOperation !== ''}
  operation={stepUpOperation}
  {tr}
  onClose={cancelStepUp}
  onAuthorized={runAuthorizedOperation}
/>
