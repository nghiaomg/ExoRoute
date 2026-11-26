<script lang="ts">
  import Button from '../../../src/components/ui/Button.svelte';
  export let onclick: () => void;
</script>

<Button type="button" variant="secondary" aria-label="Save" {onclick}>Save</Button>
