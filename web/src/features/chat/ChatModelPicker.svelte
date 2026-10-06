<script lang="ts">
  import ArkCombobox from '../../components/ArkCombobox.svelte';
  import type { Translate } from '../../lib/format';
  import type { WorkspaceChatModelOption } from '../../lib/types';
  import { modelDisplayName, modelProviderPrefix } from './chat.state';

  interface Props {
    tr: Translate;
    options?: WorkspaceChatModelOption[];
    disabled?: boolean;
    /** The selected `{prefix}/{model}` id, owned by the page draft. */
    modelId?: string;
    onModelIdChange: (modelId: string) => void;
  }

  let { tr, options = [], disabled = false, modelId = '', onModelIdChange }: Props = $props();

  // A `{prefix}/{vendor}/{model}` id is unreadably long in the list and the
  // pill, so the row shows the model name with the routing prefix as a muted
  // hint; the full id stays searchable and is exposed as the row tooltip.
  const items = $derived(
    options.map((option) => ({
      label: modelDisplayName(option.id),
      hint: modelProviderPrefix(option.id) || undefined,
      keywords: option.id,
      value: option.id,
    })),
  );
</script>

<div class="chat-model-picker">
  <ArkCombobox
    {items}
    value={modelId}
    {disabled}
    pill
    clearable={false}
    ariaLabel={tr('Model ({prefix}/{model})', { prefix: 'prefix', model: 'model' })}
    placeholder={tr('Select a model')}
    noOptionsText={tr('No saved models yet. Import a provider model list first.')}
    onValueChange={onModelIdChange}
  />
</div>

<style>
  .chat-model-picker {
    width: 100%;
    max-width: 360px;
  }
</style>
