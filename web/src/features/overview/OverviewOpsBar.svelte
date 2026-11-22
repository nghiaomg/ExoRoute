<script lang="ts">
  import Button from '../../components/ui/Button.svelte';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Translate } from '../../lib/format';
  import { RANGES, type RangeKey } from './overview.data';
  import RangeDropdown from './RangeDropdown.svelte';

  interface Props {
    tr: Translate;
    locale: string;
    endpoint: string;
    range: RangeKey;
    live: boolean;
    loading: boolean;
    updatedAt: Date | null;
    onRangeChange: (range: RangeKey) => void;
    onRefresh: () => void;
    onLiveChange: (live: boolean) => void;
  }
  let { tr, locale, endpoint, range, live, loading, updatedAt, onRangeChange, onRefresh, onLiveChange }: Props = $props();

  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  async function copyEndpoint(): Promise<void> {
    try {
      await navigator.clipboard.writeText(endpoint);
      copied = true;
      showDashboardToast(tr('Endpoint copied'));
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 1800);
    } catch {
      showDashboardToast(tr('Clipboard unavailable'));
    }
  }
</script>

<section class="overview-ops" aria-label={tr('Gateway operations')}>
  <button type="button" class="ops-endpoint" onclick={copyEndpoint} title={endpoint} aria-label={tr('Copy endpoint')}>
    <span class="ops-endpoint-label">{tr('Gateway endpoint')}</span>
    <code>{endpoint}</code>
    <svg class="icon" aria-hidden="true"><use href="#i-copy" /></svg>
    <span class="ops-copied" class:show={copied}>{copied ? tr('Copied to clipboard') : ''}</span>
  </button>
  <RangeDropdown
    {tr}
    value={range}
    label={tr('Time range')}
    options={RANGES.map((o) => ({
      value: o.key,
      label: o.key === 'CUSTOM' ? tr('Custom') : o.key,
      disabled: o.apiRange === null,
      hint: o.apiRange === null ? tr('Unavailable') : undefined,
    }))}
    onChange={(value: string) => onRangeChange(value as RangeKey)}
  />
  <div class="ops-refresh">
    <Button class="icon-button" type="button" onclick={onRefresh} disabled={loading} aria-label={tr('Refresh')} title={tr('Refresh')}>
      <svg class="icon" class:animate-spin={loading} aria-hidden="true"><use href="#i-refresh" /></svg>
    </Button>
    <span class="ops-updated">
      {#if updatedAt}{tr('Last updated {time}', { time: new Intl.DateTimeFormat(locale === 'vi' ? 'vi-VN' : 'en-US', { hour: '2-digit', minute: '2-digit', second: '2-digit' }).format(updatedAt) })}{:else}—{/if}
    </span>
  </div>
  <label class="ops-live">
    <input type="checkbox" class="ops-switch-input" role="switch" aria-checked={live} aria-label={tr('Auto-refresh')} checked={live} onchange={(event) => onLiveChange(event.currentTarget.checked)} />
    <span class="ops-switch" class:on={live} aria-hidden="true"><i></i></span>
    <span>{tr('Auto-refresh')}</span>
  </label>
</section>
