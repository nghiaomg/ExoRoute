<script lang="ts">
  import ArkDialog from '../../components/ArkDialog.svelte';
  import type { Translate } from '../../lib/format';
  import type { ConfigHelp } from './operational-settings.config';

  export let tr: Translate;
  export let help: ConfigHelp | null = null;
  export let onClose: () => void = () => {};
</script>

<ArkDialog
  open={help !== null}
  closeLabel={tr('Close dialog')}
  title={help ? tr(help.titleKey) : ''}
  kicker={tr('Operational configuration guide')}
  onClose={onClose}
>
  {#if help}
    <div class="config-help-modal">
      <div class="config-help-desc-box">
        <p>{tr(help.descKey)}</p>
      </div>

      <div class="config-help-violation-box">
        <div class="violation-header">
          <span class="violation-badge">{help.httpStatus}</span>
          <code class="violation-code">{help.errorCode}</code>
        </div>
        <div class="violation-body">
          <strong class="violation-label">{tr('Violation:')}</strong>
          <span class="violation-text">{tr(help.violationKey)}</span>
        </div>
      </div>

      <div class="config-help-meta-row">
        <div class="meta-item">
          <span class="meta-label">{tr('Default:')}</span>
          <strong class="meta-val">{help.defaultVal}</strong>
        </div>
        <div class="meta-item">
          <span class="meta-label">{tr('Allowed range:')}</span>
          <strong class="meta-val">{help.safeRange}</strong>
        </div>
      </div>

      <div class="modal-actions">
        <button type="button" class="primary-button" onclick={onClose}>
          {tr('Understood')}
        </button>
      </div>
    </div>
  {/if}
</ArkDialog>

