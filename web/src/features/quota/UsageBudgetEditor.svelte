<script lang="ts">
  import { onDestroy } from 'svelte';
  import { LoaderCircle, Save } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';
  import { localizedError } from '../../lib/errors';
  import type { ProviderKey } from '../../lib/types';
  import { ApiError, api } from '../../lib/api';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';

  export let providerId: string;
  export let key: ProviderKey;
  export let tr: Translate;
  export let onSaved: () => void;

  let open = false;
  let fiveHour = '';
  let sevenDay = '';
  let thirtyDay = '';
  let saving = false;
  let errorMessage = '';
  let notice = '';
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  function formatBudget(micros: number | null | undefined): string {
    return micros === null || micros === undefined ? '' : String(micros / 1_000_000);
  }

  function beginEdit(): void {
    fiveHour = formatBudget(key.usage_budget_5h_micros);
    sevenDay = formatBudget(key.usage_budget_7d_micros);
    thirtyDay = formatBudget(key.usage_budget_30d_micros);
    errorMessage = '';
    open = true;
  }

  function closeDialog(): void {
    if (saving) return;
    open = false;
    errorMessage = '';
  }

  function parseBudget(value: string): number | null {
    const trimmed = value.trim();
    if (!trimmed) return null;
    const parsed = Number(trimmed);
    if (!Number.isFinite(parsed) || parsed <= 0) throw new Error('Budgets must be positive USD amounts.');
    return parsed;
  }

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (saving) return;
    errorMessage = '';
    notice = '';
    saving = true;
    try {
      const result = await api.updateProviderUsageBudget(providerId, key.id, {
        five_hour_usd: parseBudget(fiveHour),
        seven_day_usd: parseBudget(sevenDay),
        thirty_day_usd: parseBudget(thirtyDay),
      });
      key.usage_budget_5h_micros = result.budget_usd['5h'] === null ? null : Math.round(result.budget_usd['5h'] * 1_000_000);
      key.usage_budget_7d_micros = result.budget_usd['7d'] === null ? null : Math.round(result.budget_usd['7d'] * 1_000_000);
      key.usage_budget_30d_micros = result.budget_usd['30d'] === null ? null : Math.round(result.budget_usd['30d'] * 1_000_000);
      notice = tr('Local budgets saved and applied immediately.');
      open = false;
      onSaved();
      if (noticeTimer) clearTimeout(noticeTimer);
      noticeTimer = setTimeout(() => { notice = ''; }, 4000);
    } catch (error) {
      errorMessage = error instanceof ApiError ? localizedError(error, 'Could not save local usage budgets.', tr) : tr('Could not save local usage budgets.');
    } finally {
      saving = false;
    }
  }

  onDestroy(() => {
    if (noticeTimer) clearTimeout(noticeTimer);
  });
</script>

<button type="button" class="secondary-button compact provider-budget-toggle" onclick={beginEdit}>
  {tr('Set local USD budgets')}
</button>
{#if notice}<small class="provider-usage-note" role="status">{notice}</small>{/if}

<ArkDialog
  {open}
  closeLabel={tr('Close dialog')}
  title={tr('Set local USD budgets')}
  kicker={key.name || tr('EXOROUTE CONTROL PLANE')}
  onClose={closeDialog}
>
  <form class="modal-form" onsubmit={save}>
    <p class="modal-description">
      {tr('Observed Cline cost compared with an optional ExoRoute USD budget. This does not report ClinePass quota.')}
    </p>

    <ArkField
      label={`${tr('5h')} (USD)`}
      type="number"
      min="0.000001"
      step="0.01"
      bind:value={fiveHour}
      placeholder={tr('No limit')}
    />

    <ArkField
      label={`${tr('7 days rolling')} (USD)`}
      type="number"
      min="0.000001"
      step="0.01"
      bind:value={sevenDay}
      placeholder={tr('No limit')}
    />

    <ArkField
      label={`${tr('30 days rolling')} (USD)`}
      type="number"
      min="0.000001"
      step="0.01"
      bind:value={thirtyDay}
      placeholder={tr('No limit')}
    />

    {#if errorMessage}
      <div class="form-error" role="alert">{errorMessage}</div>
    {/if}

    <div class="modal-actions" style="margin-top: 16px;">
      <button type="button" class="secondary-button" disabled={saving} onclick={closeDialog}>
        {tr('Cancel')}
      </button>
      <button type="submit" class="primary-button" disabled={saving}>
        {#if saving}<LoaderCircle size={15} class="spin" />{:else}<Save size={15} />{/if}
        {tr('Save budgets')}
      </button>
    </div>
  </form>
</ArkDialog>
