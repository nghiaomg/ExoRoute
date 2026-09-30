<script lang="ts">
  import ArkCombobox from '../../components/ArkCombobox.svelte';
  import type { Translate } from '../../lib/format';
  import type { WorkspaceChatModelOption } from '../../lib/types';

  interface Props {
    tr: Translate;
    options?: WorkspaceChatModelOption[];
    disabled?: boolean;
    /** The selected `{prefix}/{model}` id, owned by the page draft. */
    modelId?: string;
    onModelIdChange: (modelId: string) => void;
  }

  let { tr, options = [], disabled = false, modelId = '', onModelIdChange }: Props = $props();

  const items = $derived(options.map((option) => ({ label: option.id, value: option.id })));
</script>

<div class="chat-model-picker">
  <ArkCombobox
    {items}
    value={modelId}
    {disabled}
    pill
    ariaLabel={tr('Model (prefix/model)', { prefix: 'prefix', model: 'model' })}
    placeholder={tr('Select a model')}
    noOptionsText={tr('No saved models yet. Import a provider model list first.')}
    onValueChange={onModelIdChange}
  />
</div>

<style>
  .chat-model-picker {
    width: 250px;
    max-width: 46vw;
  }
</style>
