<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { LoaderCircle, RotateCcw, Save, Sparkles } from '@lucide/svelte';
  import ArkCheckbox from '../../components/ArkCheckbox.svelte';
  import { ApiError, api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type {
    OutputStyleId,
    OutputStyleLevel,
    OutputStyleSelection,
    OutputStylesSnapshot,
  } from '../../lib/types';

  export let tr: Translate;
  export let refreshToken = 0;

  const catalog: Array<{ id: OutputStyleId; label: string; description: string }> = [
    {
      id: 'terse-prose',
      label: 'Terse prose',
      description: 'Drop filler, articles, and hedging while keeping technical substance exact.',
    },
    {
      id: 'less-code',
      label: 'Less code',
      description: 'YAGNI ladder: smallest working change, no unrequested abstractions.',
    },
    {
      id: 'ponytail',
      label: 'Ponytail (lazy senior dev)',
      description: 'Lazy senior-dev discipline: climb the YAGNI ladder, fix root cause, smallest working diff.',
    },
  ];

  const emptyDraft: Record<OutputStyleId, OutputStyleLevel | null> = {
    'terse-prose': null,
    'less-code': null,
    ponytail: null,
  };

  let snapshot: OutputStylesSnapshot | null = null;
  let draft: Record<OutputStyleId, OutputStyleLevel | null> = { ...emptyDraft };
  let busy: '' | 'loading' | 'saving' | 'resetting' = 'loading';
  let errorMessage = '';
  let successMessage = '';
  let reloadConflict = false;
  let active = true;
  let requestGeneration = 0;
  let observedRefreshToken = 0;
  let selectedStyles: OutputStyleSelection[] = [];

  $: selectedStyles = catalog.flatMap((item) => {
    const level = draft[item.id];
    return level ? [{ id: item.id, level }] : [];
  });
  $: hasChanges = snapshot !== null && !sameSelections(selectedStyles, snapshot.styles);

  $: if (refreshToken > observedRefreshToken) {
    observedRefreshToken = refreshToken;
    void load();
  }

  function sameSelections(left: OutputStyleSelection[], right: OutputStyleSelection[]): boolean {
    return left.length === right.length
      && left.every((selection, index) => selection.id === right[index]?.id && selection.level === right[index]?.level);
  }

  function applySnapshot(result: OutputStylesSnapshot): void {
    snapshot = result;
    draft = { ...emptyDraft };
    for (const selection of result.styles) draft[selection.id] = selection.level;
  }

  function toggleStyle(id: OutputStyleId, enabled: boolean): void {
    draft = { ...draft, [id]: enabled ? (draft[id] ?? 'full') : null };
    errorMessage = '';
    successMessage = '';
  }

  function setLevel(id: OutputStyleId, level: OutputStyleLevel): void {
    draft = { ...draft, [id]: level };
    errorMessage = '';
    successMessage = '';
  }

  async function load(): Promise<void> {
    const currentRequest = ++requestGeneration;
    busy = 'loading';
    errorMessage = '';
    successMessage = '';
    reloadConflict = false;
    try {
      const result = await api.outputStyles();
      if (!active || currentRequest !== requestGeneration) return;
      applySnapshot(result);
    } catch (error) {
      if (!active || currentRequest !== requestGeneration) return;
      errorMessage = localizedError(error, 'Could not load output styles.', tr);
    } finally {
      if (active && currentRequest === requestGeneration) busy = '';
    }
  }

  async function reloadLatest(): Promise<void> {
    busy = 'loading';
    errorMessage = '';
    try {
      const result = await api.outputStyles();
      if (!active) return;
      applySnapshot(result);
      reloadConflict = false;
      successMessage = tr('Latest output styles loaded.');
    } catch (error) {
      if (active) errorMessage = localizedError(error, 'Could not load output styles.', tr);
    } finally {
      if (active) busy = '';
    }
  }

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (busy || !snapshot || !hasChanges) return;
    const requestedStyles = selectedStyles.map((selection) => ({ ...selection }));
    const expectedRevision = snapshot.revision;
    busy = 'saving';
    errorMessage = '';
    successMessage = '';
    reloadConflict = false;
    try {
      const result = await api.updateOutputStyles(requestedStyles, expectedRevision);
      if (!active) return;
      applySnapshot(result);
      successMessage = tr('Output styles saved and applied immediately.');
    } catch (error) {
      if (!active) return;
      if (error instanceof ApiError && error.status === 0) {
        try {
          const latest = await api.outputStyles();
          if (!active) return;
          snapshot = latest;
          if (sameSelections(latest.styles, requestedStyles)) {
            applySnapshot(latest);
            successMessage = tr('Output styles were saved; the response was interrupted.');
          } else {
            reloadConflict = latest.revision !== expectedRevision;
            errorMessage = localizedError(error, 'Could not save output styles.', tr);
          }
        } catch {
          errorMessage = localizedError(error, 'Could not save output styles.', tr);
        }
      } else {
        errorMessage = localizedError(error, 'Could not save output styles.', tr);
        reloadConflict = error instanceof ApiError && error.status === 409;
        if (reloadConflict) {
          try {
            snapshot = await api.outputStyles();
          } catch {
            // Keep the draft and conflict state so the user can retry loading.
          }
        }
      }
    } finally {
      if (active) busy = '';
    }
  }

  async function resetToDefault(): Promise<void> {
    if (busy || !snapshot || !snapshot.overridden) return;
    const expectedRevision = snapshot.revision;
    busy = 'resetting';
    errorMessage = '';
    successMessage = '';
    reloadConflict = false;
    try {
      const result = await api.resetOutputStyles(expectedRevision);
      if (!active) return;
      applySnapshot(result);
      successMessage = tr('Output styles reset and disabled immediately.');
    } catch (error) {
      if (!active) return;
      if (error instanceof ApiError && error.status === 0) {
        try {
          const latest = await api.outputStyles();
          if (!active) return;
          if (latest.revision > expectedRevision && !latest.overridden && latest.styles.length === 0) {
            applySnapshot(latest);
            successMessage = tr('Output styles reset successfully; the response was interrupted.');
          } else {
            snapshot = latest;
            reloadConflict = latest.revision !== expectedRevision;
            errorMessage = localizedError(error, 'Could not reset output styles.', tr);
          }
        } catch {
          errorMessage = localizedError(error, 'Could not reset output styles.', tr);
        }
      } else {
        errorMessage = localizedError(error, 'Could not reset output styles.', tr);
        reloadConflict = error instanceof ApiError && error.status === 409;
      }
    } finally {
      if (active) busy = '';
    }
  }

  onMount(() => { void load(); });
  onDestroy(() => {
    active = false;
    requestGeneration += 1;
  });
</script>

<section class="settings-panel settings-wide resource-limits-panel output-styles-panel">
  <div class="panel-heading">
    <span class="panel-icon violet-panel"><Sparkles size={17} /></span>
    <div>
      <h2>{tr('Output styles')}</h2>
      <p>{tr('Shape model responses with static instructions. Provider responses are never rewritten.')}</p>
    </div>
  </div>

  {#if busy === 'loading' && snapshot === null}
    <div class="resource-limits-state" aria-live="polite"><LoaderCircle size={15} class="spin" />{tr('Loading output styles…')}</div>
  {:else if errorMessage && snapshot === null}
    <div class="resource-limits-state resource-limits-error" role="alert">
      <span>{errorMessage}</span>
      <button class="secondary-button compact" type="button" onclick={load}>{tr('Retry')}</button>
    </div>
  {:else if snapshot}
    <div class="resource-limits-content output-styles-content">
      <p class="resource-limits-description">{tr('Output styles are disabled by default. Select any combination; instructions are always English.')}</p>
      <p class="resource-limits-description">{tr('Output style instructions are added once to the system/developer context. Security, irreversible, clarification, and ordered-sequence requests keep normal detail.')}</p>

      <div class="resource-limits-active output-styles-active">
        <strong>{tr('Active source')}</strong>
        <span>{snapshot.overridden ? tr('Database override') : tr('Default settings')} · {tr('Revision {revision}', { revision: snapshot.revision })} · {snapshot.styles.length ? snapshot.styles.map((selection) => `${tr(catalog.find((item) => item.id === selection.id)?.label ?? selection.id)} (${tr(selection.level[0].toUpperCase() + selection.level.slice(1))})`).join(', ') : tr('Disabled')}</span>
      </div>

      {#if errorMessage}
        <div class="resource-limits-error" role="alert">
          <span>{errorMessage}</span>
          {#if reloadConflict}<button class="secondary-button compact" type="button" onclick={reloadLatest}>{tr('Reload latest output styles')}</button>{/if}
        </div>
      {/if}
      {#if successMessage}<div class="resource-limits-success" role="status">{successMessage}</div>{/if}

      <form class="output-styles-form" onsubmit={save}>
        <div class="output-styles-list">
          {#each catalog as item}
            <div class="output-style-row">
              <ArkCheckbox
                checked={draft[item.id] !== null}
                ariaLabel={tr(item.label)}
                onCheckedChange={(checked) => toggleStyle(item.id, checked === true)}
              />
              <div class="output-style-copy">
                <strong>{tr(item.label)}</strong>
                <small>{tr(item.description)}</small>
              </div>
              <label class="output-style-level" for={`output-style-${item.id}`}>
                <span>{tr('style level')}</span>
                <select
                  id={`output-style-${item.id}`}
                  value={draft[item.id] ?? 'full'}
                  disabled={busy !== '' || draft[item.id] === null}
                  onchange={(event) => setLevel(item.id, (event.currentTarget as HTMLSelectElement).value as OutputStyleLevel)}
                >
                  <option value="lite">{tr('Lite')}</option>
                  <option value="full">{tr('Full')}</option>
                  <option value="ultra">{tr('Ultra')}</option>
                </select>
              </label>
            </div>
          {/each}
        </div>

        <div class="resource-limits-actions">
          <button class="primary-button" type="submit" disabled={busy !== '' || !hasChanges}>
            {#if busy === 'saving'}<LoaderCircle size={14} class="spin" />{tr('Saving…')}{:else}<Save size={14} />{tr('Save output styles')}{/if}
          </button>
          <button class="secondary-button" type="button" onclick={resetToDefault} disabled={busy !== '' || !snapshot.overridden}>
            {#if busy === 'resetting'}<LoaderCircle size={14} class="spin" />{tr('Resetting…')}{:else}<RotateCcw size={14} />{tr('Reset to default')}{/if}
          </button>
        </div>
      </form>
    </div>
  {/if}
</section>
