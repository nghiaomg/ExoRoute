<script lang="ts">
  import Button from '../../components/ui/Button.svelte';
  import type { Translate } from '../../lib/format';
  import type { DashboardPage } from '../../lib/navigation';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  interface Props { tr: Translate; providers: number; combos: number; requests: number; endpoint: string; onNavigate: (page: DashboardPage) => void; onDismiss: () => void; }
  let { tr, providers, combos, requests, endpoint, onNavigate, onDismiss }: Props = $props();
  const steps = $derived([
    { key: 'provider', label: tr('Connect an AI provider'), done: providers > 0, action: tr('Connect provider'), page: 'providers' as DashboardPage },
    { key: 'gateway', label: tr('Create an API key or routing combo'), done: combos > 0, action: tr('Create API key'), page: 'api-keys' as DashboardPage },
    { key: 'request', label: tr('Send your first request'), done: requests > 0, action: tr('Open chat playground'), page: 'chat' as DashboardPage },
  ]);
  const completed = $derived(steps.filter((s) => s.done).length);
  async function copyEndpoint(): Promise<void> {
    try { await navigator.clipboard.writeText(endpoint); showDashboardToast(tr('Copied to clipboard')); }
    catch { showDashboardToast(tr('Clipboard unavailable')); }
  }
</script>
<section class="overview-quickstart" aria-label={tr('Quickstart setup guide')}>
  <div class="qs-head">
    <div><h2>{tr('Quickstart setup guide')}</h2><p>{tr('Setup progress')}: {completed}/{steps.length} · {completed === steps.length ? tr('Setup complete') : tr('A clear view of your gateway, at a glance.')}</p></div>
    <Button class="icon-button" type="button" aria-label={tr('Dismiss')} title={tr('Dismiss')} onclick={onDismiss}>×</Button>
  </div>
  <ol class="qs-steps">
    {#each steps as step, index (step.key)}
      <li class:done={step.done}>
        <span class="qs-marker" aria-hidden="true">{step.done ? '✓' : index + 1}</span>
        <span class="qs-label">{step.label}</span>
        {#if !step.done}<Button class="mini-add" type="button" onclick={() => onNavigate(step.page)}>{step.action} →</Button>{:else}<span class="qs-done-label">{tr('Step')} {index + 1} ✓</span>{/if}
      </li>
    {/each}
  </ol>
  <div class="qs-foot"><code>{endpoint}</code><Button class="mini-add" type="button" onclick={copyEndpoint}>{tr('Copy gateway endpoint')}</Button></div>
</section>
<style>
  .overview-quickstart { padding:18px 20px; border:1px solid var(--accent); border-radius:16px; background:var(--surface); }
  .qs-head { display:flex; align-items:flex-start; justify-content:space-between; gap:10px; }
  .qs-head h2 { margin:0 0 3px; font-size:15px; letter-spacing:0; color:var(--ink); }
  .qs-head p { margin:0; font-size:12px; color:var(--muted); letter-spacing:0.01em; }
  .qs-steps { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); gap:10px; margin:14px 0 0; padding:0; list-style:none; }
  .qs-steps li { display:flex; flex-wrap:wrap; align-items:center; gap:8px; padding:10px 12px; border:1px solid var(--line); border-radius:12px; background:var(--surface-sunken); }
  .qs-marker { display:grid; place-items:center; width:22px; height:22px; flex:0 0 22px; border-radius:50%; border:1px solid var(--line); color:var(--muted); font-size:11px; font-weight:700; }
  .qs-steps li.done .qs-marker { border-color:var(--chart-good); color:var(--chart-good); }
  .qs-label { flex:1; min-width:100px; font-size:12px; color:var(--ink); letter-spacing:0.01em; }
  .qs-done-label { font-size:11px; color:var(--chart-good); }
  .qs-foot { display:flex; flex-wrap:wrap; align-items:center; gap:10px; margin-top:12px; padding-top:12px; border-top:1px solid var(--line); }
  .qs-foot code { flex:1; min-width:180px; font-family:var(--font-mono); font-size:11px; color:var(--accent); }
  @media (max-width:900px){ .qs-steps { grid-template-columns:1fr; } }
</style>
