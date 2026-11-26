<script lang="ts">
  // Chat-page UI tests replace the Ark combobox with this plain input: the
  // real picker drives a portal-based dropdown jsdom cannot run, and the
  // turn tests exercise sending/stopping/streaming, not picker navigation.
  import type { Translate } from '../../../src/lib/format';
  import type { WorkspaceChatModelOption } from '../../../src/lib/types';
  import { modelDisplayName } from '../../../src/features/chat/chat.state';

  interface Props {
    tr: Translate;
    options?: WorkspaceChatModelOption[];
    disabled?: boolean;
    modelId?: string;
    onModelIdChange: (modelId: string) => void;
  }

  let { tr, options = [], disabled = false, modelId = '', onModelIdChange }: Props = $props();

  let query = $state('');
  const filtered = $derived(
    query.trim()
      ? options.filter((option) => option.id.toLowerCase().includes(query.trim().toLowerCase()))
      : options,
  );
</script>

<input
  class="model-picker-stub"
  type="text"
  placeholder={tr('Select a model')}
  {disabled}
  value={modelId ? modelDisplayName(modelId) : query}
  oninput={(event) => (query = event.currentTarget.value)}
  onkeydown={(event) => {
    if (event.key !== 'Enter') return;
    const match = filtered[0];
    if (match) {
      onModelIdChange(match.id);
      query = '';
    }
  }}
/>
