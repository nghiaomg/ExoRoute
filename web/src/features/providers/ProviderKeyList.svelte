<script lang="ts">
  import { ArrowDown, ArrowUp, Check, KeyRound, Pencil, Power, PowerOff, Trash2, X } from '@lucide/svelte';
  import { type Locale } from '../../lib/i18n';
  import { formatDate, type Translate } from '../../lib/format';
  import type { ProviderKey } from '../../lib/types';

  export let keys: ProviderKey[];
  export let tr: Translate;
  export let locale: Locale;
  export let deletingId = '';
  export let onRemove: (key: ProviderKey) => void;
  export let onRename: ((key: ProviderKey, name: string) => void) | undefined = undefined;
  export let onToggle: ((key: ProviderKey, enabled: boolean) => void) | undefined = undefined;
  export let onMove: ((key: ProviderKey, direction: -1 | 1) => void) | undefined = undefined;
  export let busyId = '';
  export let positionOffset = 0;
  export let hasPreviousPage = false;
  export let hasNextPage = false;

  let renamingId = '';
  let renameDraft = '';

  const deferredClineApiKeyTest = 'Cline API keys are saved without an automatic inference test; use Test model explicitly.';

  function isDeferredClineApiKeyTest(key: ProviderKey): boolean {
    return key.credential_type === 'api_key'
      && key.last_test_passed === false
      && key.last_error === deferredClineApiKeyTest;
  }

  function keyErrorMessage(message: string): string {
    const openRouterStatus = message.match(/^OpenRouter API key could not be verified \(HTTP (\d+)\)$/);
    if (openRouterStatus) return tr('OpenRouter API key could not be verified (HTTP {status}).', { status: openRouterStatus[1] });
    const commandCodeStatus = message.match(/^Command Code API key could not be verified \(HTTP (\d+)\)$/);
    if (commandCodeStatus) return tr('Command Code API key could not be verified (HTTP {status}).', { status: commandCodeStatus[1] });
    return message;
  }

  function startRename(key: ProviderKey): void {
    renamingId = key.id;
    renameDraft = key.name;
  }

  function commitRename(key: ProviderKey): void {
    const name = renameDraft.trim();
    renamingId = '';
    if (!name || name === key.name || !onRename) return;
    onRename(key, name);
  }

  function canMove(index: number, direction: -1 | 1): boolean {
    return direction === -1 ? index > 0 || hasPreviousPage : index < keys.length - 1 || hasNextPage;
  }
</script>

<div class="provider-key-list">
  {#each keys as key, index (key.id)}
    <article class="provider-key-item">
      <div class="provider-key-info">
        {#if onMove}<span class="provider-key-position" title={tr('Fallback position {position}', { position: positionOffset + index + 1 })}>{positionOffset + index + 1}</span>{/if}
        <span class="provider-key-icon"><KeyRound size={14} /></span>
        {#if renamingId === key.id}
          <form
            class="provider-key-rename"
            onsubmit={(event) => {
              event.preventDefault();
              commitRename(key);
            }}
          >
            <input bind:value={renameDraft} maxlength={256} aria-label={tr('Key name')} />
            <button type="submit" class="row-icon" aria-label={tr('Save key name')} title={tr('Save key name')}><Check size={14} /></button>
            <button type="button" class="row-icon" aria-label={tr('Cancel')} title={tr('Cancel')} onclick={() => (renamingId = '')}><X size={14} /></button>
          </form>
        {:else}
          <div>
            <strong>{key.name}</strong>
            {#if key.credential_type}<small class="provider-key-credential-type">{tr(key.credential_type === 'oauth' ? 'OAuth account' : 'API key')}</small>{/if}
            <small>{tr('Created')} {formatDate(key.created_at, locale)}{key.last_used_at ? ` · ${tr('Last used')} ${formatDate(key.last_used_at, locale)}` : ''}</small>
            {#if (key.invalid || key.last_test_passed === false) && key.last_error}<small class="provider-key-error">{keyErrorMessage(key.last_error)}</small>{/if}
          </div>
        {/if}
      </div>
      <div class="provider-key-controls">
        <span class="provider-key-status" class:invalid={key.invalid || (key.last_test_passed === false && !isDeferredClineApiKeyTest(key))}>
          {tr(key.invalid ? 'Invalid' : key.credential_type === 'oauth' ? key.enabled ? 'Connected' : 'Disabled' : isDeferredClineApiKeyTest(key) ? 'Not tested' : key.last_test_passed === false ? 'Test failed' : key.enabled ? 'Ready' : 'Disabled')}
        </span>
        {#if onMove}
          <button class="row-icon" aria-label={tr('Move “{name}” up in the fallback order', { name: key.name })} title={tr('Move up')} disabled={busyId === key.id || !canMove(index, -1)} onclick={() => onMove(key, -1)}><ArrowUp size={14} /></button>
          <button class="row-icon" aria-label={tr('Move “{name}” down in the fallback order', { name: key.name })} title={tr('Move down')} disabled={busyId === key.id || !canMove(index, 1)} onclick={() => onMove(key, 1)}><ArrowDown size={14} /></button>
        {/if}
        {#if onToggle}
          <button class="row-icon" aria-label={tr(key.enabled ? 'Disable key “{name}”' : 'Enable key “{name}”', { name: key.name })} title={tr(key.enabled ? 'Disable' : 'Enable')} disabled={busyId === key.id} onclick={() => onToggle(key, !key.enabled)}>
            {#if key.enabled}<PowerOff size={14} />{:else}<Power size={14} />{/if}
          </button>
        {/if}
        {#if onRename}
          <button class="row-icon" aria-label={tr('Rename key “{name}”', { name: key.name })} title={tr('Rename')} disabled={busyId === key.id} onclick={() => startRename(key)}><Pencil size={14} /></button>
        {/if}
        <button class="row-icon danger-hover" aria-label={tr('Remove provider key “{name}”', { name: key.name })} title={tr('Remove provider key “{name}”', { name: key.name })} disabled={deletingId === key.id || busyId === key.id} onclick={() => onRemove(key)}><Trash2 size={14} /></button>
      </div>
    </article>
  {/each}
</div>
